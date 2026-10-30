# Gaps

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (rewritten: only open shortfalls, ranked, with estimates; finished work moved to the revision history) · **Target:** Cisco Packet Tracer 9.0

Every known shortfall against the reference app, ranked: the work list. Pick from the top; each
item lands with tests and doc updates. The weights behind the ranking are in
[target-app-parity.md](target-app-parity.md); detail per area in
[protocol-parity.md](protocol-parity.md), [ui-parity.md](ui-parity.md),
[hardware-parity.md](hardware-parity.md) and [localization-parity.md](localization-parity.md).
Hours are Opus 5.5 agent wall-clock estimates.

## Ranked

| # | Gap | Who notices | Estimate |
|---|---|---|---|
| 1 | **Activity depth:** a side-by-side view of the student's lab against the answer network | Instructors | 5–8 h |
| 2 | **Wireless depth:** WLC with lightweight APs, 802.1X/RADIUS, 802.11 frames in Simulation mode | CCNA 2 | 16–26 h |
| 3 | **Services:** SNMP traps and v3, relaying mail between servers | Network management labs | 6–10 h |
| 4 | **More languages and full coverage:** event summaries in Spanish; more interface languages | Non-English students | 6–10 h |
| 5 | **Packaging and first release:** installers for Windows, macOS, FreeBSD and Flatpak, workflows, contributors in the About window ([releasing.md](releasing.md)); the app ID and the project's home are the owner's decision | Everyone who isn't a developer | 10–18 h |
| 6 | **Workspace:** drawing freeform, clusters, physical view, direct printing | Everyone, occasionally | 45–70 h |
| 7 | **Device families:** modular routers, ASA, IoT, phones | Specialised labs | 80–120 h |
| 8 | **EIGRP, VPN (GRE/IPsec), QoS** | CCNA 3 extras | 40–60 h |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Services narrowed: SNMP and the MIB Browser. |
| 2026-10-30 | minor | Services narrowed: TACACS+. |
| 2026-10-30 | minor | Languages narrowed: engine errors and drop reasons in Spanish. |
| 2026-10-30 | minor | Workspace narrowed: File ▸ Export Workspace Image (PNG). |
| 2026-10-30 | minor | Wireless narrowed: the home router's Internet port takes DHCP (`ip address dhcp` on routers too). |
| 2026-10-30 | minor | Services narrowed: email (SMTP, POP3, the Email app). |
| 2026-10-30 | minor | Activities narrowed: values per student (variables picked from the student's name), results per section. |
| 2026-10-30 | minor | Done: console coverage (the CCNA 1 lab commands: output filters, password rules, speed and duplex, many `show` commands). |
| 2026-10-30 | minor | Wireless narrowed: the Home-Router (DHCP, NAT, Wi-Fi, settings page); NAT, DHCP server and ACLs on multilayer switches. |
| 2026-10-30 | minor | Languages narrowed: messages with parts and the sample labs in Spanish. |
| 2026-10-30 | minor | Packaging narrowed: app icon, Linux desktop entry and installer. |
| 2026-10-30 | minor | Console narrowed: `debug ip packet`, `debug ip nat`, `debug ip dhcp server events`. |
| 2026-10-30 | minor | Services narrowed: FTP. |
| 2026-10-30 | minor | Services narrowed: AAA with RADIUS. |
| 2026-10-30 | minor | Done: IPv6 depth (OSPFv3 added). |
| 2026-10-30 | minor | IPv6 depth narrowed: DHCPv6 relay. |
| 2026-10-30 | minor | IPv6 depth narrowed: IPv6 on subinterfaces, VLAN interfaces and multilayer switches. |
| 2026-10-30 | minor | Done: OSPF stub and totally stubby areas, `area range`, RIP on multilayer switches (NSSA left out: beyond CCNA). |
| 2026-10-30 | minor | Done: protocol frames in Simulation mode (OSPF database exchange added; CDP, DTP and PAgP formats aren't public). |
| 2026-10-30 | minor | Gap 3 narrowed: LACPDUs. |
| 2026-10-30 | minor | Gap 2 narrowed: `debug ip rip`, `debug ip ospf`. |
| 2026-10-30 | minor | Gaps 2 and 3 narrowed: LLDP with frames, `show processes`, `show memory`. |
| 2026-10-30 | minor | Gap 1 narrowed: time limit, hints per check, password lock, score-only results. |
| 2026-10-30 | minor | Done: Spanish interface (catalog, Options ▸ Language). |
| 2026-10-30 | minor | Done: Wi-Fi access points and laptops (SSID, WPA2-PSK, range, relay between clients), wifi sample lab. |
| 2026-10-30 | minor | Done: spanning-tree listening/learning timers, rapid-pvst. |
| 2026-10-30 | minor | Done: the web build asks before closing a tab with unsaved changes; File ▸ Open Recent on the desktop. |
| 2026-10-30 | minor | Done: NTP, syslog and TFTP (server services and router commands). |
| 2026-10-30 | minor | Done: DHCPv6 (stateless and stateful) and router advertisement M/O flags. |
| 2026-10-30 | minor | Done: OSPF cost, priority and DR/BDR, reference bandwidth, inter-area routes, `show ip ospf [interface]`, OSPF on multilayer switches. |
| 2026-10-30 | minor | Done: ACL depth (`access-class` on vty lines, IPv6 access lists and traffic filters, NAT for ICMP errors). |
| 2026-10-30 | minor | Done: PPP PAP/CHAP authentication. |
| 2026-10-30 | minor | Done: DTP (dynamic auto/desirable, nonegotiate, negotiated working mode). |
| 2026-10-30 | minor | Done: HSRP (groups, priority, preempt, virtual MAC, hellos, `show standby`, hsrp sample). |
| 2026-10-30 | minor | Gap 1 narrowed: activities with instructions, checks (configuration, ping, host address, command output), points, hidden answers, Check Results, Reset Activity and an Activity Wizard. |
| 2026-10-30 | minor | Gap 2 narrowed: logging buffer, console messages, `debug ip icmp`. |
| 2026-10-30 | minor | Gap 3 narrowed: BPDUs, OSPF hellos and RIP updates in the event list and captures. |
| 2026-10-30 | minor | Gap 2 narrowed: CDP, clock, flash, users, terminal, `show ip interface`, `show interfaces status/switchport`, `show running-config interface`, richer `show version` done. |
| 2026-10-30 | major | Rewritten as a ranked table of open gaps with estimates; finished items moved here. |
| 2026-10-29 | minor | Done: serial WAN links (Router-WAN, DCE/DTE, clock rate, HDLC/PPP, `show controllers`). |
| 2026-10-29 | minor | Done: RIP (versions 1/2, classful networks, passive interfaces, split horizon, default origination). |
| 2026-10-29 | minor | Done: multilayer switch (`ip routing`, routed ports, static routes). |
| 2026-10-29 | minor | Done: IPv6 (addresses, NDP, SLAAC, static routes, ICMPv6, PC IPv6 configuration). |
| 2026-10-29 | minor | Done: EtherChannel (LACP, PAgP, static), VTP, port security. |
| 2026-10-29 | minor | Done: Telnet and SSH between devices, local users, `login local`. |
| 2026-10-29 | minor | Done: server services (HTTP, DNS, DHCP) and the PC web browser. |
| 2026-10-29 | minor | Done: DHCP server and relay, spanning tree, switch VLAN interfaces, OSPF, ACLs and NAT. |
| 2026-10-29 | major | Created. |
