# Gaps

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (Resize Shape done; devices as accessible nodes, M9.8) · also merged with M9.6 (results verified by replay) · earlier: minor (item 1 narrowed after seven troubleshooting labs) · also merged with M9.2 and M9.3 (routing timers; real DES, 3DES, AH and elliptic-curve Diffie-Hellman) · earlier: minor (code health: one modular-exponentiation engine), major (the lab book, automation depth, security, instructor, Simulation-mode, switching and routing, wireless and device-family items closed; what is honestly left re-ranked) · **Target:** Cisco Packet Tracer 9.0

Every known shortfall against the reference app, ranked: the work list. Pick from the top; each
item lands with tests and doc updates. The weights behind the ranking are in
[target-app-parity.md](target-app-parity.md); detail per area in
[protocol-parity.md](protocol-parity.md), [ui-parity.md](ui-parity.md),
[hardware-parity.md](hardware-parity.md) and [localization-parity.md](localization-parity.md).
Hours are Opus 5.5 agent wall-clock estimates.

## Ranked

| # | Gap | Who notices | Estimate |
|---|---|---|---|
| 1 | **Console depth beyond the lab book:** the book's 27 labs run (7 of them troubleshooting labs), but the courses have many more activities. Left: more labs of our own on the topics the troubleshooting labs did not touch (EIGRP, IPv6 routing, EtherChannel, port security, VPN and wireless faults, voice, `debug` output), and the details already seen but not done: OSPF authentication (`ip ospf authentication`, message-digest keys), `show ip ospf database router\|network\|summary`, `show ip route summary`, `show ip nat translations verbose`, `show ip protocols summary`, `show spanning-tree brief\|detail\|interface`, ICMP type and code in ACLs (`permit icmp any any 3 4`), ACL counters following their line when lines are edited (they are keyed by position), `debug ip ospf adj\|events` printing the same text as `hello`, and `show interfaces` lacking the `line protocol` wording students filter on | Every student | 8–16 h |
| 2 | **Crypto stand-ins:** the IKE SA itself is never rekeyed (child SAs are), AH next to ESP in one transform set keeps the stand-in cipher (AH on its own is real, in tunnel mode), and IKEv1 group 24 keeps a stand-in exchange | CCNA 3 and security courses | 2–4 h |
| 3 | **Certificates and PKI:** TLS under RESTCONF uses an anonymous Diffie-Hellman suite with no certificate, the SSH host key is an HMAC stand-in for a signature, PEAP names its server instead of showing a certificate and uses a toy 61-bit group; no `crypto pki`, trustpoints or RSA signatures | CCNA 3 automation and security labs | 6–10 h |
| 4 | **Cellular network:** smartphones are Wi-Fi only; no cell towers, central office server or mobile data | IoT and mobile-device labs | 6–10 h |
| 5 | **IoT and boards depth:** the climate things have no physics (a heater or air conditioner doesn't change a thermostat's reading), BoardScript's compile and run messages are English only, and boards run BoardScript rather than Python or JavaScript ([decisions.md](decisions.md)) | IoT labs | 4–8 h |
| 6 | **Workspace leftovers:** device windows as native OS windows (and cables as accessible nodes) | Everyone, screen-reader users | 8–14 h |
| 7 | **More end devices:** TVs, a sniffer, more router models | Specialised labs | 4–8 h |
| 8 | **More languages:** interface languages beyond English and Spanish (catalogs in the same format) | Non-English students | 3–6 h each |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | Item 6 narrowed again (M9.8): canvas devices are accessible nodes (label with the project's model name and selection, Tab and arrow keys select, Enter or a screen reader's click opens). Left: device windows as native OS windows (not attempted: egui viewports are not available on the web build and the in-app windows hold per-window state), and cables as nodes. |
| 2026-11-13 | minor | Item 6 narrowed (M9.8): Resize Shape is done (`note.resize`, handles on a selected rectangle, ellipse or line, bounded, one undo step per drag). Left: canvas devices as accessible nodes, device windows as native OS windows. |
| 2026-11-13 | minor | Done: results that resist forgery (old item 8, M9.6): a JSON results file (version 2) embeds the student's final network and `activity.verify_results` replays the activity's checks against it, comparing the score, points per item and answers with the claim. CSV and older files are verified for hand edits only and say so; time claims are not checked ([decisions.md](decisions.md)). Languages move up to item 8. |
| 2026-11-13 | minor | Item 1 narrowed (M9.4): seven troubleshooting labs (21 to 27) walked and run as written; 12 gaps found and fixed (`show interfaces switchport` for all ports and `show interfaces X trunk`, `no ip proxy-arp`, `show ip ospf neighbor` by router ID or interface, `show ip route NET MASK`, the ACL `log` keyword and its messages, `clear (ip) access-list counters NAME`, `show ip dhcp pool NAME` with the addresses left, NAT without `overload` sharing one address, NAT hits and misses with `clear ip nat statistics`, NAT rules counting matches on their ACL, a deleted ACL's counters reset, `show spanning-tree root\|bridge\|blockedports`). What is left is in the item. |
| 2026-11-13 | minor | Done: routing leftovers (old item 5), last part (M9.2): the master of a VRRPv3 IPv6 group announces the virtual router with router advertisements (from the virtual link-local address and virtual MAC, when it takes the role and every 200 s, and in answer to router solicitations; a backup stays quiet). GLBP hellos on the wire are dropped from the list: the format is not public, so none are planned ([protocol-parity.md](protocol-parity.md)). Later items move up one place. |
| 2026-11-13 | minor | Done (part of old item 5): EIGRP for IPv6 per-interface timers (M9.2): `ipv6 hello-interval eigrp AS S` and `ipv6 hold-time eigrp AS S`, hellos at the interface's interval announcing its hold time, a silent neighbour kept until its hold time runs out and then dropped with a log line and DUAL's queries. Still open in item 5: VRRPv3 router advertisements, GLBP hellos. |
| 2026-11-13 | minor | Done (part of old item 5): RIPng timers, hold-down and triggered updates (M9.2): `timers basic` under `ipv6 router rip`, silent neighbours kept until the invalid time, possibly-down routes at metric 16 with hold-down and flush, triggered responses a second after a change. Still open in item 5: EIGRP for IPv6 hello and hold timers, VRRPv3 router advertisements, GLBP hellos. |
| 2026-11-13 | minor | Narrowed: the elliptic-curve groups 19 (P-256) and 20 (P-384) (item 2) are real: ECDH of RFC 5903 in `wire/src/ec.rs` over the existing big integers, checked against the RFC 5903 section 8 vectors (both curves, both directions), peer points validated (on the curve, below the prime, not infinity). IKEv1 and IKEv2 use it. Left: IKE SA rekeying, AH bundled with ESP, IKEv1 group 24 (a stand-in exchange; the console accepts it), transport mode in the data path. |
| 2026-11-13 | minor | Narrowed: AH (item 2) is real: RFC 4302 in `wire/src/ah.rs` (header, ICV over the IPv4 header with its mutable fields zeroed per §3.3.3, tunnel and transport framing) with HMAC-SHA1-96, HMAC-MD5-96 and HMAC-SHA-256-128, negotiated by IKEv1 quick mode and IKEv2 child SAs (protocol 2) and carried as IP protocol 51 in tunnel mode when a transform set holds only `ah-*` transforms. Left: AH bundled with ESP in one set, and transport mode in the data path (the simulator tunnels everything it protects). |
| 2026-11-13 | minor | Narrowed: DES and 3DES (item 2) are real: FIPS 46-3 and SP 800-67 in `wire/src/des.rs`, CBC for IKEv1 (`encryption des\|3des`), IKEv2 (ENCR_3DES) and ESP (`esp-des`, `esp-3des`), tested against published known-answer vectors. AH, the elliptic-curve groups and IKE SA rekeying are left. |
| 2026-11-13 | minor | Done: code health (old item 11): one modular-exponentiation engine. `wire/src/modp.rs` (TLS and SSH, group 14) is now a thin layer over `wire/src/dh.rs`, which also builds the prime, so the duplicate Montgomery code and the hard-coded prime are gone. |
| 2026-11-13 | minor | Done: packaging and first release: Linux (AppImage, deb, rpm, tar.gz), Windows (MSI, portable zip), macOS (dmg) and web packages built by `release.yml`, SHA256SUMS and a draft release; builds are not signed yet ([releasing.md](releasing.md)). |
| 2026-10-31 | major | Re-ranked after the round below: console depth beyond the lab book first, then the crypto stand-ins, certificates, a cellular network, routing, IoT, workspace, end-device, instructor and language leftovers, and the duplicated modular exponentiation. |
| 2026-10-31 | minor | Done: switching and routing leftovers (old item 6): CoS 802.1p with trust and maps, VRRP timers, master-down, skew and tracking, VRRPv3 for IPv6, EIGRP for IPv6 updates, queries and replies on the wire with variance and summaries (IPv4 summaries too), RIP timers, hold-down, triggered updates and poisoned reverse, RIPng, VTP pruning (computed). What is left is the new item 5. |
| 2026-10-31 | minor | Done: device families (old item 8): Tablet and Smartphone (Wi-Fi only), ten Wi-Fi IoT things, Board SBC and Board MCU with BoardScript and the IoT Custom cable, the phone's Settings screen, power supplies. What is left is the new items 4, 6 and 8. |
| 2026-10-31 | minor | Done: wireless leftovers (old item 7): a real PEAP tunnel with EAP-GTC and EAP over RADIUS (Message-Authenticator, MPPE keys), central switching through CAPWAP data. Certificates are in the new item 3. |
| 2026-10-31 | minor | Done: Simulation-mode leftovers (old item 5): Constant Delay, the capture filter, the sender's decisions in the OSI view, absolute PDU start times. |
| 2026-10-31 | minor | Done: instructor tools (old item 4): answer network sealed in the activity, assessment tree, initial network, instructions per language, connectivity tests, signed results export, activity locks. What is left is the new item 9. |
| 2026-10-31 | minor | Done: security leftovers (old item 3): real Diffie-Hellman over MODP groups 1, 2, 5, 14, 15 and 16, AES-128/192/256, SHA-256 and HMAC-SHA-256-128, IKEv1 keys and AES-CBC, real ESP for AES with SHA-1, SHA-256 or MD5, SA lifetimes and child-SA rekeying, default MTU 1500 and GRE 1476 with path MTU discovery, FTP, DNS and HTTP inspection. What is left is the new item 2. |
| 2026-10-31 | minor | Done: automation depth (old item 2): XML in RESTCONF with Accept and Content-Type, IPv6 data and transport, NETCONF candidate with commit, discard and validate, locks and kept sessions, TLS 1.2 and SSH 2.0 underneath, controller discovery over CDP and LLDP, inventory, templated pushes. Certificates are in the new item 3. |
| 2026-10-31 | minor | Done: the CCNA lab book (old item 1, beta gate 1): 20 labs in `docs/labs/` run end to end by the engine, 8 over MCP, with the fixes they turned up. More labs are the new item 1. |
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
