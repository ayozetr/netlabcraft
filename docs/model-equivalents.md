# Model equivalents

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (cellular radio on Tablet and Smartphone, Cell Tower and Central Office Server) · earlier: minor (Tablet, Smartphone, ten Wi-Fi IoT things, Board SBC and Board MCU; power-supply bays) · **Target:** CCNA course labs

Course labs name specific vendor models. NetLabCraft's models are its own, named after their
ports (`GE` Gigabit Ethernet, `FE` Fast Ethernet, `S` serial), and never carry vendor names
([decisions.md](decisions.md)). This table says which NetLabCraft model to place when a lab
asks for a given model, so the same commands and port numbers work. It only describes
equivalence for teaching; the vendor names below are mentioned to identify the products a lab
refers to.

Lab files keep the stable model id (second column); the workspace shows the name (first
column). Commands accept either: `device.add {"model": "Router 3GE"}` or `{"model": "Router-3"}`.

Routers and switches have one or two power-supply bays at the back with one supply fitted from
the factory (`device.psu`; the Router 2GE, Router 2GE+2S, Modular Router 2GE, Firewall 5GE,
Switch 8FE, Switch 8FE PoE and WLC have one bay, the other routers and switches two). Where a
lab has students fit a redundant supply, use a two-bay model.

| NetLabCraft model | Id in files | Use it where a lab asks for | Port names | Notes |
|---|---|---|---|---|
| Router 2GE | `Router-2` | 1941, 2901, ISR 4321 | `GigabitEthernet0/0–0/1` | A 4321 names its ports `0/0/0–0/0/1`. |
| Router 3GE | `Router-3` | 2911, ISR 4331 | `GigabitEthernet0/0–0/2` | The usual CCNA router. A 4331 names its ports `0/0/0–0/0/2`. |
| Router 4GE | `Router-4` | Routers with four Gigabit ports | `GigabitEthernet0/0–0/3` | |
| Firewall 5GE | `Firewall-5` | ASA 5506-X | `GigabitEthernet1/1–1/5` | Firewall commands (`nameif`, `security-level`, object NAT). A 5506-X has eight ports (`1/1–1/8`): use the first five. |
| Modular Router 2GE | `Router-M2` | 1941 with its module bays | `GigabitEthernet0/0–0/1`, cards in bays `0/0–0/1` | Fit cards in its Physical tab with the power off. |
| Modular Router 3GE | `Router-M3` | 2911 with its module bays | `GigabitEthernet0/0–0/2`, cards in bays `0/0–0/3` | |
| 2-port Serial card | `Card-2S` | HWIC-2T | `Serial0/B/0–0/B/1` (B = bay) | |
| 4-port Serial card | `Card-4S` | HWIC-4T | `Serial0/B/0–0/B/3` | |
| 1-port Gigabit card | `Card-1GE` | EHWIC-1GE-SFP-CU | `GigabitEthernet0/B/0` | |
| 1-port Gigabit fiber card | `Card-1GF` | SFP-based Gigabit cards | `GigabitEthernet0/B/0` (fiber) | |
| 1-port Fast Ethernet card | `Card-1FE` | HWIC-1FE | `FastEthernet0/B/0` | |
| 4-port switch card | `Card-4SW` | HWIC-4ESW, EHWIC-4ESG | `FastEthernet0/B/0–0/B/3` | Layer-2 ports inside the router: VLANs, `switchport access\|trunk`, routed through `interface vlan N`. |
| Router 2GE+2S | `Router-WAN` | 1941 or 2911 with a 2-port serial card (HWIC-2T), ISR 4321 with a serial NIM | `GigabitEthernet0/0–0/1`, `Serial0/0/0–0/0/1` | Serial ports are fitted from the start. |
| Switch 8FE | `Switch-8` | Small 8-port access switches | `FastEthernet0/1–0/8`, `GigabitEthernet0/1` | |
| Switch 24FE | `Switch-24` | 2960-24TT | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` | The usual CCNA switch. |
| Switch 24FE+2SFP | `Switch-24-SFP` | 2960 models with SFP uplinks | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` (fiber) | The other switches' Gigabit uplinks take copper or fiber. |
| Switch 48FE | `Switch-48` | 48-port 2960 models | `FastEthernet0/1–0/48`, `GigabitEthernet0/1–0/2` | |
| Switch 8FE PoE | `Switch-8-PoE` | Small PoE access switches | `FastEthernet0/1–0/8`, `GigabitEthernet0/1` | Power over Ethernet on the Fast Ethernet ports, 65 W in all. |
| Switch 24FE PoE | `Switch-24-PoE` | 2960-24LT-L; 3560-24PS used as a layer-2 switch | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` | Power over Ethernet, 370 W in all. It doesn't route: for `ip routing` use L3 Switch 24 (which has no PoE). |
| L3 Switch 24 | `Switch-ML-24` | 3560-24PS, 3650-24PS | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` | Routes with `ip routing`. A 3650 has Gigabit access ports (`GigabitEthernet1/0/1…`): use the same numbers on `FastEthernet0/…`. |
| Access Point | `AccessPoint` | AccessPoint-PT, autonomous access points | `GigabitEthernet0`, `Wireless0` | |
| WLC | `WLC` | WLC-2504, 3504 | `GigabitEthernet0/1–0/4` | WLANs are set with `wlan …`, not a web page. |
| Light AP | `LAP` | Lightweight APs (3702i and similar) | `GigabitEthernet0`, `Wireless0` | Joins a WLC. Needs a PoE switch port or its power adapter. |
| Home Router | `Home-Router` | WRT300N, HomeRouter-PT | `Internet0`, `Ethernet1–4`, `Wireless0` | |
| Home Gateway | `Home-Gateway` | The home gateway in the course's IoT labs | `Internet0`, `Ethernet1–4`, `Wireless0` | A home router with a built-in IoT service and an IoT tab. |
| Cloud | `Cloud` | Cloud-PT | `Ethernet0–1`, `Coaxial0–1`, `Phone0–1` | Ports joined in pairs (`cloud.map`; coaxial to phone by default), shown in its Physical tab. |
| DSL Modem | `DSL-Modem` | DSL-Modem-PT | `Phone0`, `Ethernet1` | Bridges the phone line to Ethernet. |
| Cable Modem | `Cable-Modem` | Cable-Modem-PT | `Coaxial0`, `Ethernet1` | Bridges the coaxial cable to Ethernet. |
| Hub 8 | `Hub-8` | Hub-PT | `Ethernet0–7` | |
| PC | `PC` | PC-PT | `FastEthernet0` | |
| Laptop | `Laptop` | Laptop-PT | `FastEthernet0` | |
| Tablet | `Tablet` | TabletPC-PT | `Wireless0`, `Cellular0` | Wi-Fi, or mobile data through a cell tower; a touch home screen with the Desktop's apps. |
| Smartphone | `Smartphone` | SMARTPHONE-PT | `Wireless0`, `Cellular0` | Wi-Fi, or mobile data through a cell tower (Wi-Fi is preferred when connected); a touch home screen with the Desktop's apps. |
| Cell Tower | `Cell-Tower` | Cell Tower | `Cellular0–5` (sectors), `Ethernet0` (backhaul) | A radio with a coverage range; mobile devices in range attach to it. |
| Central Office Server | `Central-Office` | Central Office Server | `GigabitEthernet0/0–2` | The provider's data service: admits attached mobile devices, gives each an address from its pool and routes them to the lab (`cellular pool\|apn\|data-plan`). |
| Wi-Fi Laptop | `Laptop-WiFi` | Laptop-PT with a wireless card | `Wireless0` | |
| IP Phone | `IP-Phone` | 7960 | `FastEthernet0` (network), `FastEthernet1` (PC) | Registers with `telephony-service` on a router, or, set to SIP (Signal with SIP, `phone.configure`), with `voice register`. Needs a PoE switch port or its power adapter. |
| Smart Sensor | `IoT-Sensor` | The temperature monitor among the course's IoT things | `FastEthernet0` | Reports to a server's IoT service or a Home Gateway. |
| Smart Device | `IoT-Device` | The ceiling fan among the course's IoT things | `FastEthernet0` | Switched by the rules or by hand. |
| Motion Detector | `IoT-Motion` | The course's motion detector | `FastEthernet0` | Reports 1 while something moves, else 0. |
| Smoke Detector | `IoT-Smoke` | The course's smoke detector | `FastEthernet0` | Reports the smoke level (0–100 %). |
| Smart Light | `IoT-Light` | The course's smart lamp or light | `FastEthernet0` | Switched on and off. |
| Smart Door Lock | `IoT-Door` | The course's smart door | `FastEthernet0` | On unlocks, off locks. |
| Siren | `IoT-Siren` | The course's siren | `FastEthernet0` | Sounds while on. |
| Webcam | `IoT-Webcam` | The course's webcam | `FastEthernet0` | Takes a still image at each report while on. |
| Thermostat | `IoT-Thermostat` | The course's thermostat | `Wireless0` | Reports degrees. Wi-Fi things join the home gateway's `HomeNetwork` when placed. |
| Heater | `IoT-Heater` | The course's heater (furnace) | `Wireless0` | Switched on and off; doesn't change a thermostat's reading. |
| Air Conditioner | `IoT-AC` | The course's air conditioner | `Wireless0` | Switched on and off; doesn't change a thermostat's reading. |
| Humidity Sensor | `IoT-Humidity` | The course's humidity monitor | `Wireless0` | Reports % humidity. |
| Smart Window | `IoT-Window` | The course's window | `Wireless0` | On opens, off closes. |
| Soil Moisture Sensor | `IoT-Soil` | The course's soil moisture sensor | `Wireless0` | Reports % moisture. |
| Lawn Sprinkler | `IoT-Sprinkler` | The course's lawn sprinkler | `Wireless0` | Waters while on. |
| CO Detector | `IoT-CO` | The course's carbon monoxide detector | `Wireless0` | Reports ppm CO. |
| Smart Plug | `IoT-Plug` | The course's power outlet | `Wireless0` | Powered while on. |
| Garage Door | `IoT-Garage` | The course's garage door | `Wireless0` | On opens, off closes. |
| Board SBC | `Board-SBC` | SBC boards | `FastEthernet0`, pins `D0–D7`, `A0–A3` | Runs BoardScript, not Python or JavaScript: course programs have to be written again in it. IoT custom cables join pins to things. |
| Board MCU | `Board-MCU` | MCU boards | `Wireless0`, pins `D0–D5`, `A0–A1` | Runs BoardScript, as the Board SBC. |
| Server | `Server` | Server-PT | `GigabitEthernet0` | |
| Printer | `Printer` | Printer-PT | `FastEthernet0` | |

Models a lab names that have no equivalent yet (the cellular network for smartphones, cell
towers and the central office; TVs, a sniffer and other end devices) are listed in
[hardware-parity.md](hardware-parity.md).

Cisco, Catalyst, ISR and Packet Tracer are trademarks of Cisco Systems, Inc. NetLabCraft is an
independent project, not affiliated with or endorsed by Cisco.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | Cellular radio on Tablet and Smartphone; Cell Tower and Central Office Server rows. |
| 2026-10-31 | minor | Tablet and Smartphone, ten Wi-Fi IoT things, Board SBC and Board MCU (BoardScript, not Python or JavaScript); power-supply bays per model; the "no equivalent yet" list now names cellular, TVs and the sniffer. |
| 2026-10-30 | minor | 4-port switch card, the two PoE switches, Home Gateway, Cloud, DSL and cable modems, six more IoT things; IP Phone with SIP and power; the "no equivalent yet" list brought up to date. |
| 2026-10-30 | minor | Switch 24FE+2SFP and the fiber card. |
| 2026-10-30 | minor | Modular routers and their cards, IP Phone, Smart Sensor and Smart Device. |
| 2026-10-30 | minor | Firewall 5GE. |
| 2026-10-30 | major | Created: which NetLabCraft model to place for each model course labs name. |
