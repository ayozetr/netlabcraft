# Model equivalents

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (created) · **Target:** CCNA course labs

Course labs name specific vendor models. PacketCraft's models are its own, named after their
ports (`GE` Gigabit Ethernet, `FE` Fast Ethernet, `S` serial), and never carry vendor names
([decisions.md](decisions.md)). This table says which PacketCraft model to place when a lab
asks for a given model, so the same commands and port numbers work. It only describes
equivalence for teaching; the vendor names below are mentioned to identify the products a lab
refers to.

Lab files keep the stable model id (second column); the workspace shows the name (first
column). Commands accept either: `device.add {"model": "Router 3GE"}` or `{"model": "Router-3"}`.

| PacketCraft model | Id in files | Use it where a lab asks for | Port names | Notes |
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
| 1-port Fast Ethernet card | `Card-1FE` | HWIC-1FE | `FastEthernet0/B/0` | |
| Router 2GE+2S | `Router-WAN` | 1941 or 2911 with a 2-port serial card (HWIC-2T), ISR 4321 with a serial NIM | `GigabitEthernet0/0–0/1`, `Serial0/0/0–0/0/1` | Serial ports are fitted from the start. |
| Switch 8FE | `Switch-8` | Small 8-port access switches | `FastEthernet0/1–0/8`, `GigabitEthernet0/1` | |
| Switch 24FE | `Switch-24` | 2960-24TT | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` | The usual CCNA switch. |
| Switch 48FE | `Switch-48` | 48-port 2960 models | `FastEthernet0/1–0/48`, `GigabitEthernet0/1–0/2` | |
| L3 Switch 24 | `Switch-ML-24` | 3560-24PS, 3650-24PS | `FastEthernet0/1–0/24`, `GigabitEthernet0/1–0/2` | Routes with `ip routing`. A 3650 has Gigabit access ports (`GigabitEthernet1/0/1…`): use the same numbers on `FastEthernet0/…`. |
| Access Point | `AccessPoint` | AccessPoint-PT, autonomous access points | `GigabitEthernet0`, `Wireless0` | |
| WLC | `WLC` | WLC-2504, 3504 | `GigabitEthernet0/1–0/4` | WLANs are set with `wlan …`, not a web page. |
| Light AP | `LAP` | Lightweight APs (3702i and similar) | `GigabitEthernet0`, `Wireless0` | Joins a WLC. |
| Home Router | `Home-Router` | WRT300N, HomeRouter-PT | `Internet0`, `Ethernet1–4`, `Wireless0` | |
| Hub 8 | `Hub-8` | Hub-PT | `Ethernet0–7` | |
| PC | `PC` | PC-PT | `FastEthernet0` | |
| Laptop | `Laptop` | Laptop-PT | `FastEthernet0` | |
| Wi-Fi Laptop | `Laptop-WiFi` | Laptop-PT with a wireless card | `Wireless0` | |
| IP Phone | `IP-Phone` | 7960 | `FastEthernet0` (network), `FastEthernet1` (PC) | Registers with `telephony-service` on a router. |
| Smart Sensor | `IoT-Sensor` | The temperature monitor among the course's IoT things | `FastEthernet0` | Reports to a server's IoT service. |
| Smart Device | `IoT-Device` | The ceiling fan among the course's IoT things | `FastEthernet0` | Switched by the server's rules or by hand. |
| Server | `Server` | Server-PT | `GigabitEthernet0` | |
| Printer | `Printer` | Printer-PT | `FastEthernet0` | |

Models a lab names that have no equivalent yet (modular slots, IP phones, IoT devices, cloud,
DSL/cable modems) are listed in [hardware-parity.md](hardware-parity.md).

Cisco, Catalyst, ISR and Packet Tracer are trademarks of Cisco Systems, Inc. PacketCraft is an
independent project, not affiliated with or endorsed by Cisco.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Modular routers and their cards, IP Phone, Smart Sensor and Smart Device. |
| 2026-10-30 | minor | Firewall 5GE. |
| 2026-10-30 | major | Created: which PacketCraft model to place for each model course labs name. |
