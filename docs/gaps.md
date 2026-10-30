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
| 1 | **Activity depth:** a timer, locked fields (students can't open the wizard or see answers in the UI), variable values per student, feedback text per check, an answer network to compare against | Instructors | 30–50 h |
| 2 | **Console coverage:** more debugs, `show processes/memory`, LLDP, `show ip ospf interface`; going through the CCNA 1 labs command by command | CCNA 1 from the first lab | 6–14 h |
| 3 | **More protocol frames in Simulation mode:** OSPF database exchange (DBD/LSR/LSU), LACP, LLDP; BPDUs, OSPF hellos and RIP updates are done | CCNA 2–3 students watching Simulation mode | 8–14 h |
| 4 | **Wireless LANs:** access points, wireless routers, laptop wireless cards, SSID and WPA2, association | CCNA 2 | 40–60 h |
| 5 | **Spanning-tree timers and RSTP** (listening/learning delays, proposal/agreement) | CCNA 2 | 8–12 h |
| 6 | **OSPF database on the wire and area types:** DBD/LSR/LSU/LSAck exchange in Simulation mode, stub/NSSA areas, summarisation; RIP on multilayer switches | CCNA 3 | 14–21 h |
| 7 | **IPv6 depth:** OSPFv3, DHCPv6 relay, IPv6 on subinterfaces and VLAN interfaces | CCNA 2–3 | 28–40 h |
| 8 | **Services:** FTP, email (SMTP/POP3 with the PC Email app), SNMP, AAA (RADIUS/TACACS+) | CCNA 3, network management labs | 40–60 h |
| 9 | **Translations** through `craft-i18n`, Spanish first | Non-English students | 25–40 h |
| 10 | **Packaging and first release:** icon, installers, workflows, About window ([releasing.md](releasing.md)) | Everyone who isn't a developer | 14–24 h |
| 11 | **Workspace:** drawing freeform, clusters, physical view, print/export the workspace | Everyone, occasionally | 50–75 h |
| 12 | **Device families:** modular routers, ASA, IoT, phones | Specialised labs | 80–120 h |
| 13 | **EIGRP, VPN (GRE/IPsec), QoS** | CCNA 3 extras | 40–60 h |

## Revision history

| Date | Change | Summary |
|---|---|---|
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
