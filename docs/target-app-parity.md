# Target-app parity: NetLabCraft vs Cisco Packet Tracer

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (audiences re-estimated per course with a written method; hours made consistent; counts measured again) · **Target:** Cisco Packet Tracer 9.0 (as used in CCNA courses)

This is the authoritative parity assessment. [`ROADMAP.md`](../ROADMAP.md) summarizes it,
[`gaps.md`](gaps.md) lists every shortfall, and the area checklists hold the detail:
[protocols and commands](protocol-parity.md), [UI](ui-parity.md), [hardware](hardware-parity.md),
[localization](localization-parity.md). The generated command list is
[`command-reference.md`](command-reference.md).

## Summary

| Number | Value | Kind |
|---|---|---|
| Engine commands | **94** | measured: `c!(` entries in `crates/engine/src/catalog.rs` |
| Console grammar entries | **802** across 26 modes | measured: `spec!(` rows in `crates/ios/src/grammar.rs`; modes are its `Modes` constants less `ANY_MODE`, `BOTH` and the device flags `ROUTER`, `SWITCH`, `FIREWALL` |
| Device models | **25** | measured: `MODELS` in `crates/model/src/models.rs` (the fallback model and the 6 router cards not counted) |
| Sample labs | **23** | measured: `SAMPLES` in `crates/engine/src/sample.rs` |
| Tests | **336** | measured: sum of "passed" from `cargo test --workspace -q` |
| **Ready for real work** (weighted) | **~84%** | estimated; written weights below (83.6 computed) |
| **CCNA 1 student** (Introduction to Networks) | **~86%** | estimated; [method](#audiences) |
| **CCNA 2 student** (Switching, Routing and Wireless Essentials) | **~85%** | estimated; [method](#audiences) |
| **CCNA 3 student** (Enterprise Networking, Security and Automation) | **~79%** | estimated; [method](#audiences) |
| **Instructor** (builds graded activities) | **~78%** | estimated; activities with checks, points, sections, hints, hidden answers, values per student, a time limit, a password lock, a wizard and a side-by-side comparison with the answer |
| Stage | **pre-alpha** | see [Stage](#stage) |
| Remaining to alpha | **~5–10 Opus 5.5 agent-hours** | estimated; the packaged-build gate is on hold ([roadmap.md](roadmap.md#alpha-gates)) |
| Remaining to ~95% in every area | **~150–250 Opus 5.5 agent-hours** | estimated; sum of the area rows below |

### Readiness by audience

| Audience | Ready % | Opus 5.5 agent wall-clock hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (the weighted total) | ~84% | ~150–250 (sum of the area rows) | Simulation-mode depth, device families, automation, security depth |
| CCNA 1 student | ~86% | ~10–20 | PDU details in the OSI layout, complex PDUs, walking the labs |
| CCNA 2 student | ~85% | ~20–35 | Wireless group keys and real PEAP, loop guard, the RSTP handshake, option 82, walking the labs |
| CCNA 3 student | ~79% | ~35–55 | Automation (RESTCONF/NETCONF, a Programming tab), zone-based firewall, QoS trust and shaping, `ip mtu`/`adjust-mss` |
| Instructor | ~78% | ~15–25 | Translated activity texts, an answer-network view |

Each student tier is mostly a superset of the one before. Hours are wall-clock for one agent;
the protocol work splits well across 3–4 agents by crate (`sim`, `ios`, `ui-egui`). The audience
hours overlap each other and the area rows, so they don't add up to the total.

## What we measured against, and how

- **Target:** Cisco Packet Tracer 9.0.1, the version on the owner's machine, as used in the
  Cisco Networking Academy CCNA courses.
- **Clean-room limits.** The project's absolute rule ([`AGENTS.md`](../AGENTS.md)) forbids reading,
  disassembling or decompiling anything inside the Packet Tracer installation, its `.pkt`/`.pka`
  files or its protocols. The reference was observed **black-box only**: running it with
  synthetic labs and taking screenshots that stay in the local `plan/packet-tracer/screenshots/`
  folder (never committed). Protocol behaviour comes from the IETF RFCs and IEEE standards named
  in [`protocol-parity.md`](protocol-parity.md).
- **Feature list:** the course topics of the three CCNA courses, as public in the course outlines,
  and the menus, tabs and device windows observed in the running app.
- **Measured numbers** are counted from source; **estimated** numbers are judgement against the
  weights below and say so.

## Areas and weights

Weights reflect how much of a typical CCNA lab session each area carries. **How the totals are
computed:** points = weight × ready; the weighted total is the sum of the points (the "ready for
real work" and "full target" figure); the hours to ~95% in every area are the sum of the hours
column. [`ROADMAP.md`](../ROADMAP.md)'s dimension rows are the same rows with the same numbers.

| Area | Weight | Ready | Points | Detail | Hours to ~95% |
|---|---|---|---|---|---|
| Device console (modes, commands, `show` output, help, completion) | 20 | 82% | 16.4 | [protocol-parity.md](protocol-parity.md#console) | 25–40 |
| Layer 2 (VLANs, trunks, STP, EtherChannel, VTP, port security, DTP, snooping, ARP inspection) | 12 | 90% | 10.8 | [protocol-parity.md](protocol-parity.md#layer-2) | 6–10 |
| Layer 3 (static, RIP, OSPF, EIGRP, IPv6, multilayer switching, FHRP) | 15 | 92% | 13.8 | [protocol-parity.md](protocol-parity.md#layer-3) | 10–16 |
| Services (DHCP, DNS, HTTP, FTP, email, NTP, syslog, AAA, SNMP) | 8 | 94% | 7.5 | [protocol-parity.md](protocol-parity.md#services) | 4–8 |
| Security (ACLs, NAT, SSH, passwords, AAA, PPP authentication, VPN, firewall) | 8 | 86% | 6.9 | [protocol-parity.md](protocol-parity.md#security) | 15–25 |
| WAN, cabling and wireless | 6 | 87% | 5.2 | [hardware-parity.md](hardware-parity.md) | 12–20 |
| Simulation mode (event list, PDU details, filters, complex PDUs) | 8 | 70% | 5.6 | [ui-parity.md](ui-parity.md) | 18–28 |
| Workspace and windows (canvas, device tabs, physical view, drawing) | 10 | 80% | 8.0 | [ui-parity.md](ui-parity.md) | 15–25 |
| Teaching (activities, scoring, instructions, languages) | 8 | 80% | 6.4 | [gaps.md](gaps.md) | 15–25 |
| Device families and hardware (models, modules, firewalls, phones, IoT) | 5 | 60% | 3.0 | [hardware-parity.md](hardware-parity.md) | 30–50 |
| **Total** | **100** | | **83.6** | | **~150–250** |

## Audiences

**Method.** Each course is split into its lab topics, weighted by their share of the course's
Packet Tracer activities (estimated from the public course outlines). Each topic is scored for
how much of what its labs do works here, depth included: a command that is accepted but not
simulated scores low. The weighted sum then loses **5 points** while no lab book has been run
end to end (beta gate 1), because untested labs always turn up surprises. The earlier figures
(CCNA 2 ~92%, CCNA 3 ~91%) counted topics as present without weighing their depth or the
untested labs; with this method the same state scores lower even after this round's work.

| CCNA 1 topic | Weight | Ready |
|---|---|---|
| Device basics, passwords, SSH | 15 | 95% |
| IPv4 addressing and hosts | 15 | 98% |
| IPv6 addressing, SLAAC, DHCPv6 | 10 | 92% |
| ARP, NDP, ping, traceroute | 10 | 95% |
| Switch basics, MAC table | 10 | 95% |
| Simulation mode and PDU details | 15 | 75% |
| Host apps (prompt, browser, email, FTP, terminal) | 10 | 90% |
| Cabling and the physical layer | 10 | 90% |
| Application services (DNS, DHCP, HTTP) | 5 | 95% |
| **Weighted, less 5** | | **91.2 − 5 ≈ 86%** |

| CCNA 2 topic | Weight | Ready |
|---|---|---|
| VLANs, trunks, DTP | 15 | 95% |
| Inter-VLAN routing (router-on-a-stick, multilayer) | 10 | 95% |
| STP and RSTP (PortFast, BPDU guard, root guard, cost, priority) | 12 | 88% |
| EtherChannel | 8 | 95% |
| DHCPv4 and relay | 8 | 95% |
| SLAAC and DHCPv6 | 7 | 85% |
| FHRP (HSRP) | 7 | 92% |
| LAN security (port security, DHCP snooping, ARP inspection) | 10 | 88% |
| Wireless LANs (WLC, WPA2) | 13 | 75% |
| Static and floating static routes, IPv4 and IPv6 | 10 | 95% |
| **Weighted, less 5** | | **90.0 − 5 ≈ 85%** |

| CCNA 3 topic | Weight | Ready |
|---|---|---|
| Single-area OSPFv2 | 18 | 95% |
| ACLs | 18 | 93% |
| NAT | 10 | 95% |
| Network management (CDP, LLDP, NTP, syslog, SNMP, backups, images, password recovery) | 14 | 92% |
| VPN and IPsec | 8 | 75% |
| QoS | 5 | 60% |
| WAN concepts | 4 | 85% |
| Design and troubleshooting labs | 15 | 90% |
| Automation (REST APIs, controllers) | 5 | 15% |
| Virtualization and SDN | 3 | 30% |
| **Weighted, less 5** | | **83.8 − 5 ≈ 79%** |

- **CCNA 1 (Introduction to Networks):** almost all of it works; what is missing is the PDU
  details in the reference app's OSI layout, complex PDUs, and a walk through the labs.
- **CCNA 2 (SRWE):** VLANs, STP with its protections, EtherChannel, DHCP, FHRP, LAN security and
  static routing work, including floating static routes. Wireless is the weakest topic (group
  keys, a real PEAP tunnel); loop guard, the RSTP handshake and option 82 insertion are missing.
- **CCNA 3 (ENSA):** OSPF as the course teaches it, ACLs (with editing), NAT, GRE/IPsec VPNs,
  QoS classification, marking, policing and queuing, SNMP, syslog, NTP, backups and password
  recovery work. Shaping is accepted but not simulated, `mls qos trust` is missing, and the
  automation topics (RESTCONF, NETCONF, a Programming tab, a controller) are not there.
- **Instructor:** builds activities with checks, a score and a side-by-side comparison with the
  answer; no answer-network view.

## Stage

**Pre-alpha:** every core workflow works end to end (build a lab, configure by typing, ping,
watch frames, grade an activity, save), and the console covers the commands of the first labs.
What keeps it pre-alpha: nobody has yet walked the CCNA 1 labs command by command to confirm it,
and the packaged-build gate is on hold by the owner's decision (no release for now). Alpha gates
are in [`roadmap.md`](roadmap.md#alpha-gates).

## Better than the reference

- **Real frames:** every frame is encoded and decoded; the event list exports to pcap for
  Wireshark or any analyser.
- **Open lab format:** `.nlcraft` is readable JSON ([`file-format.md`](file-format.md)).
- **Agent control:** every action is a command, reachable from the CLI and an MCP server.
- **Runs everywhere:** Linux, Windows, macOS, BSD and the browser, with no account or sign-in.
- **Drop reasons:** a dropped frame says why (ACL, VLAN not allowed, blocked by STP…).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | major | Audiences re-estimated per course with a written method (topics weighted by their share of the Packet Tracer activities, depth scored, 5 points off until a lab book runs): CCNA 1 ~86%, CCNA 2 ~92% → ~85%, CCNA 3 ~91% → ~79%. Console 74% → 82%, layer 2 85% → 90%, layer 3 90% → 92%, services 92% → 94%, security 85% → 86%, WAN/cabling/wireless 85% → 87% (fibre was done), workspace 78% → 80%; total 80.6 → 83.6. Hours per area redone so the total is their sum (~150–250); the separate full-parity figure dropped. Counts measured again (94 commands, 802 grammar entries across 26 modes, 25 models, 23 samples). |
| 2026-10-30 | minor | Depth round: layer 3 85% → 90%, security 78% → 85%, WAN/cabling/wireless 78% → 85%, device families 40% → 60%; total 77.9 → 80.6; CCNA 2 ~92%, CCNA 3 ~91%; counts measured again (92 commands, 636 grammar entries across 26 modes, 27 models, 23 samples). |
| 2026-10-30 | minor | Physical view: workspace 65% → 78%; firewall: device families 25% → 40%, CCNA 3 ~85% → ~87%; total 75.8 → 77.9; counts measured again (80 commands, 558 grammar entries across 22 modes, 19 models, 20 samples). |
| 2026-10-30 | minor | WLC and 802.11: WAN/cabling/wireless 62% → 78%, CCNA 2 ~84% → ~90%; QoS: layer 3 83% → 85%, CCNA 3 ~80% → ~85%; total 74.5 → 75.8; counts measured again (68 commands, 516 grammar entries across 21 modes, 18 models, 19 samples). |
| 2026-10-30 | minor | SNMP traps and v3, mail relay: services 86% → 92%, total 74.0 → 74.5. |
| 2026-10-30 | minor | File ▸ Print: workspace 63% → 65%, total 73.8 → 74.0. |
| 2026-10-30 | minor | Compare with the Answer: teaching 75% → 80%, instructor ~72% → ~78%, total 73.4 → 73.8. |
| 2026-10-30 | minor | EIGRP: layer 3 78% → 83%; GRE/IPsec: security 72% → 78%; drawings and clusters: workspace 55% → 63%; total 71.4 → 73.4, CCNA 3 ~74% → ~80%; engine commands, grammar entries and models recounted (65, 453 across 18 modes, 16); the headline row said ~50% by mistake. |
| 2026-10-30 | minor | SNMP: services 80% → 86%, CCNA 3 ~70% → ~74%. |
| 2026-10-30 | minor | TACACS+: services 77% → 80%. |
| 2026-10-30 | minor | Email: services 70% → 77%, CCNA 1 ~84% → ~86%. |
| 2026-10-30 | minor | Values per student and results per section: teaching 65% → 75%, instructor ~60% → ~72%. |
| 2026-10-30 | minor | CCNA 1 lab commands: console 68% → 74%, CCNA 1 ~80% → ~84%. |
| 2026-10-30 | minor | Audiences re-estimated after this round: CCNA 1 ~80%, CCNA 2 ~84%, CCNA 3 ~70%, full target ~58%. |
| 2026-10-30 | minor | Home wireless router: WAN, cabling and wireless 55% → 62%. |
| 2026-10-30 | minor | FTP: services 64% → 70%. |
| 2026-10-30 | minor | AAA with RADIUS: services 58% → 64%. |
| 2026-10-30 | minor | DHCPv6 relay and OSPFv3: layer 3 72% → 78%. |
| 2026-10-30 | minor | IPv6 on subinterfaces and VLAN interfaces: layer 3 70% → 72%. |
| 2026-10-30 | minor | OSPF stub areas and ranges, RIP on multilayer switches: layer 3 66% → 70%. |
| 2026-10-30 | minor | LACP and OSPF database exchange on the wire: Simulation mode 67% → 70%. |
| 2026-10-30 | minor | LLDP with frames, `show processes`, `show memory`: console 65% → 68%, Simulation mode 65% → 67%; grammar entries recounted (309). |
| 2026-10-30 | minor | Activity time limit, hints and password lock: teaching 55% → 65%, instructor ~45% → ~60%. |
| 2026-10-30 | minor | Spanish interface: teaching 45% → 55%. |
| 2026-10-30 | minor | Wi-Fi access points and clients: WAN/cabling/wireless 35% → 55%, CCNA 2 ~71% → ~78%. |
| 2026-10-30 | minor | STP timers and rapid-pvst: layer 2 80% → 85%, CCNA 2 ~68% → ~71%. |
| 2026-10-30 | minor | NTP, syslog, TFTP: services 45% → 58%, CCNA 3 ~57% → ~62%. |
| 2026-10-30 | minor | DHCPv6: services 40% → 45%, CCNA 2 ~62% → ~68%. |
| 2026-10-30 | minor | OSPF depth: layer 3 60% → 66%, CCNA 3 ~52% → ~57%. |
| 2026-10-30 | minor | access-class, IPv6 ACLs, NAT for ICMP errors: security 65% → 72%, CCNA 3 ~45% → ~52%. |
| 2026-10-30 | minor | PPP authentication: security 60% → 65%. |
| 2026-10-30 | minor | DTP: layer 2 75% → 80%. |
| 2026-10-30 | minor | HSRP: layer 3 55% → 60%, CCNA 2 ~58% → ~62%. |
| 2026-10-30 | minor | Activities (checks, points, hidden answers, wizard): teaching 5% → 45%, instructor ~12% → ~45%. |
| 2026-10-30 | minor | BPDUs, OSPF hellos and RIP updates in Simulation mode: simulation 55% → 65%, alpha ~25–45 h. |
| 2026-10-30 | minor | Console coverage (CDP, clock, flash, users, interface views): console 60% → 65%, CCNA 1 ~70% → ~74%, alpha ~30–55 h. |
| 2026-10-30 | major | Rewritten in the shared format: measured numbers, weights, audiences per CCNA course, estimates and stage. |
| 2026-10-29 | minor | Serial WAN links, RIP, multilayer switch, IPv6, EtherChannel, VTP, port security, Telnet/SSH. |
| 2026-10-29 | major | Created. |
