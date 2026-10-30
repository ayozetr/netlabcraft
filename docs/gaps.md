# Gaps

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (automation, security, Simulation-mode depth, leftovers and device families closed; what is left re-ranked) · **Target:** Cisco Packet Tracer 9.0

Every known shortfall against the reference app, ranked: the work list. Pick from the top; each
item lands with tests and doc updates. The weights behind the ranking are in
[target-app-parity.md](target-app-parity.md); detail per area in
[protocol-parity.md](protocol-parity.md), [ui-parity.md](ui-parity.md),
[hardware-parity.md](hardware-parity.md) and [localization-parity.md](localization-parity.md).
Hours are Opus 5.5 agent wall-clock estimates.

## Ranked

| # | Gap | Who notices | Estimate |
|---|---|---|---|
| 1 | **A CCNA-style lab book** (beta gate 1, **in progress**): VLANs, inter-VLAN routing, DHCP, OSPF, ACLs, NAT, IPv6, WAN, switch security and HSRP labs written fresh and run end to end by hand and through MCP, fixing what they turn up | Every student; it is also how the readiness estimates get checked | 25–40 h |
| 2 | **Automation depth (CCNA 3):** TLS under RESTCONF (port 443 carries plain HTTP today) and SSH under NETCONF, XML bodies and IPv6 addresses in RESTCONF, a candidate datastore with commit in NETCONF, a controller closer to the course's (device inventory, topology, policies) | CCNA 3 students | 10–16 h |
| 3 | **Security leftovers:** a real Diffie-Hellman exchange for IKE (a stand-in today), AES-256 and SHA-256 ESP, real ciphers for IKEv1 tunnels, rekeying, the default MTU of tunnels, more firewall inspections | CCNA 3 and security courses | 8–14 h |
| 4 | **Instructor tools:** an answer-network view in the Activity Wizard, translated activity texts | Instructors | 6–10 h |
| 5 | **Simulation-mode leftovers:** a Constant Delay option, capturing only the filtered events, the origin device's own decisions in the OSI view (only receivers record theirs), absolute start times for PDUs | CCNA 1 students | 4–8 h |
| 6 | **Switching and routing leftovers:** CoS values (frames always carry 0, so `trust cos` keeps 0), VRRP advertisement and master-down timers, EIGRP for IPv6 updates on the wire, RIP triggered updates, VTP pruning | CCNA 2 and 3 extras | 6–10 h |
| 7 | **Wireless leftovers:** a real PEAP/TLS tunnel (shown as an exchange today), central tunnelling through the controller | CCNA 2 extras | 4–8 h |
| 8 | **Device families:** tablets, smartphones and other end devices, more IoT things and programmable boards, the phone's own setup screen, power supplies as parts | Specialised labs | 15–25 h |
| 9 | **More languages:** interface languages beyond English and Spanish (catalogs in the same format) | Non-English students | 3–6 h each |
| — | **Packaging and first release:** on hold by the owner's decision (no release for now; GitHub Actions is not available). Not planned or estimated until the hold is lifted; see [releasing.md](releasing.md) | Everyone who isn't a developer | on hold |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | major | Re-ranked after the round below: the lab book stays first and is in progress; automation depth, security leftovers, instructor tools, Simulation-mode, switching/routing, wireless and device-family leftovers follow. |
| 2026-10-30 | minor | Done: device families (old item 9): a 4-port switch card in routers, PoE switches with power budgets and powered devices that need PoE or their adapter, SIP phones, six more IoT things with timed reports and AND/OR rules, a Home Gateway, a Cloud with DSL and cable modems and coaxial and phone cables. What is left is the new item 8. |
| 2026-10-30 | minor | Wireless depth (old item 7) narrowed: group keys (GTK by AES key wrap, group frames under it) and range by building in the physical view; a real PEAP tunnel is left (new item 7). |
| 2026-10-30 | minor | Done: routing leftovers (old item 6): EIGRP for IPv6, VRRP v2/v3, GLBP basics, DHCPv6 /128 addresses for router clients, HSRP preempt honoured, the OSPF network-type mismatch. Timers and hellos left (new item 6). |
| 2026-10-30 | minor | Done: switch and QoS leftovers (old item 5): `mls qos trust`, shaping that delays traffic, loop guard and BPDU filter, option 82 insertion with relay trust, the RSTP proposal/agreement handshake. CoS values left (new item 6). |
| 2026-10-30 | minor | Done: Simulation-mode depth (old item 4): complex PDUs (14 applications, one-shot or periodic), the OSI Model view of PDU details with recorded decisions, scenarios saved in the lab. Leftovers are the new item 5. |
| 2026-10-30 | minor | Done: security depth (old item 3): zone-based firewall, classic `ip inspect`, `ip mtu` with fragmentation and RFC 1191 errors, `ip tcp adjust-mss`, IKEv2, real AES-128/HMAC-SHA1-96 ESP for IKEv2. Leftovers are the new item 3. |
| 2026-10-30 | minor | Done: automation (old item 2): RESTCONF, NETCONF, the Programming app, a controller. Depth left (new item 2). |
| 2026-10-30 | major | Re-ranked: the lab book first, then automation, security depth and Simulation-mode depth; packaging put on hold by the owner's decision; fibre removed from device families (it was done with the cable types). |
| 2026-10-30 | minor | Done: console and PC prompt fixes (`^` marker, first-word help, `show ip traffic`, `show tcp brief`, extended ping, history size; `ipconfig /all`, `ipv6config /all`, `ping -l/-t`, `netstat`, `arp -d`). |
| 2026-10-30 | minor | Done: HSRP version 2 and interface tracking. |
| 2026-10-30 | minor | Done: network management (timestamps, logging levels, NTP master and authentication, TFTP image copies, `boot system`, password recovery, `show file systems`, `show ip nat statistics`). |
| 2026-10-30 | minor | Done: DHCP server details (lease, domain name, statistics, conflicts, packet debug), helper addresses and ACLs on VLAN interfaces. |
| 2026-10-30 | minor | Done: ACL editing (sequence numbers, resequence, `established`, ICMP types, remarks). |
| 2026-10-30 | minor | Done: small switch commands (`show vlan id`, `show vlan summary`, `show dtp`, `show interfaces counters`, trunk encapsulation, `sdm prefer`). |
| 2026-10-30 | minor | Done: STP cost and port priority, root guard, `show spanning-tree summary` and `detail`, EtherChannel load balancing and views. |
| 2026-10-30 | minor | Done: PortFast and BPDU guard defaults, errdisable recovery, port-security aging. |
| 2026-10-30 | minor | Done: DHCP snooping and dynamic ARP inspection (option 82 insertion stays open). |
| 2026-10-30 | minor | Done: routers as IPv6 clients (`ipv6 address autoconfig`, `ipv6 address dhcp`). |
| 2026-10-30 | minor | Routing depth narrowed: floating static routes (route distance in path choice). |
| 2026-10-30 | minor | Done: OSPF as the courses teach it (interface areas, timers with mismatch, point-to-point, `passive-interface default`, `redistribute static`, database and neighbour detail). |
| 2026-10-30 | minor | Done: loopback interfaces. |
| 2026-10-30 | minor | Workspace narrowed: UI fixes from the audit (fit to view, time bar, Spanish coverage, window placement, labels, event list columns). |
| 2026-10-30 | minor | Depth round: wireless (VLAN per WLAN, beacons, roaming, CCMP), routing and VPN (EIGRP depth, ECMP, tunnels over learned routes, IKE, QoS queuing), firewall depth, modular routers, IP phones and IoT; what remains of each re-ranked. |
| 2026-10-30 | minor | Workspace done: the physical view. Device families narrowed: a stateful firewall. |
| 2026-10-30 | minor | Wireless narrowed: WLC, LAPs, 802.11 frames, WPA2-Enterprise. QoS basics done (classification, marking, policing). |
| 2026-10-30 | minor | Services done: SNMP traps, SNMPv3 without privacy, mail relay (v3 privacy and MX records are in protocol-parity.md). |
| 2026-10-30 | minor | Workspace narrowed: File ▸ Print. |
| 2026-10-30 | minor | Languages narrowed: event summaries and PDU details in Spanish. |
| 2026-10-30 | minor | Activity depth done: Compare with the Answer, check by check. |
| 2026-10-30 | minor | Workspace narrowed: freehand and polygon drawings, clusters. |
| 2026-10-30 | minor | CCNA 3 extras narrowed: EIGRP, GRE tunnels and IPsec site-to-site; QoS and depth remain. |
| 2026-10-30 | minor | Packaging narrowed: the About window lists contributors. |
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
