# Target-app parity: PacketCraft vs Cisco Packet Tracer

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (EIGRP, GRE/IPsec, drawings and clusters; measured numbers recounted) · **Target:** Cisco Packet Tracer 9.0 (as used in CCNA courses)

This is the authoritative parity assessment. [`ROADMAP.md`](../ROADMAP.md) summarizes it,
[`gaps.md`](gaps.md) lists every shortfall, and the area checklists hold the detail:
[protocols and commands](protocol-parity.md), [UI](ui-parity.md), [hardware](hardware-parity.md),
[localization](localization-parity.md). The generated command list is
[`command-reference.md`](command-reference.md).

## Summary

| Number | Value | Kind |
|---|---|---|
| Engine commands | **65** | measured: `crates/engine/src/catalog.rs` |
| Console grammar entries | **453** across 18 modes | measured: `spec!` rows in `crates/ios/src/grammar.rs` |
| Device models | **16** | measured: `crates/model/src/models.rs` |
| Sample labs | **18** | measured: `crates/engine/src/sample.rs` |
| Tests | **197** | measured: `cargo test --workspace` |
| **Ready for real work** (weighted) | **~74%** | estimated; written weights below (73.8 computed) |
| **CCNA 1 student** (Introduction to Networks) | **~86%** | estimated; [method](#audiences) |
| **CCNA 2 student** (Switching, Routing and Wireless Essentials) | **~84%** | estimated; [method](#audiences) |
| **CCNA 3 student** (Enterprise Networking, Security and Automation) | **~80%** | estimated; [method](#audiences) |
| **Instructor** (builds graded activities) | **~78%** | estimated; activities with checks, points, sections, hints, hidden answers, values per student, a time limit, a password lock, a wizard and a side-by-side comparison with the answer |
| Stage | **pre-alpha** | see [Stage](#stage) |
| Remaining to alpha | **~25–45 Opus 5.5 agent-hours** | estimated; [roadmap.md](roadmap.md#alpha-gates) |
| Remaining to full parity | **~550–880 Opus 5.5 agent-hours** | estimated; sum of the rows below |

### Readiness by audience

| Audience | Ready % | Opus 5.5 agent wall-clock hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (ready for real work) | ~61% | ~430–700 | More device families (ASA, IoT, modules), physical workspace, QoS, WLC |
| CCNA 1 student | ~86% | ~12–25 | PDU details layout and a few host apps (text editor, terminal) |
| CCNA 2 student | ~84% | ~30–55 | WLC with lightweight APs and 802.11 frames (15–25 h), console polish |
| CCNA 3 student | ~80% | ~40–70 | QoS basics (10–15 h), automation APIs, the IKE exchange on the wire |
| Instructor | ~78% | ~15–25 | Translated activity texts, an answer-network view |

Each student tier is mostly a superset of the one before. Hours are wall-clock for one agent;
the protocol work splits well across 3–4 agents by crate (`sim`, `ios`, `ui-egui`).

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

Weights reflect how much of a typical CCNA lab session each area carries.

| Area | Weight | Ready | Points | Detail | Hours to ~95% |
|---|---|---|---|---|---|
| Device console (modes, commands, `show` output, help, completion) | 20 | 74% | 14.8 | [protocol-parity.md](protocol-parity.md#console) | 60–90 |
| Layer 2 (VLANs, trunks, STP, EtherChannel, VTP, port security, DTP) | 12 | 85% | 10.2 | [protocol-parity.md](protocol-parity.md#layer-2) | 25–40 |
| Layer 3 (static, RIP, OSPF, EIGRP, IPv6, multilayer switching) | 15 | 83% | 12.5 | [protocol-parity.md](protocol-parity.md#layer-3) | 55–90 |
| Services (DHCP, DNS, HTTP, FTP, email, NTP, syslog, AAA) | 8 | 86% | 6.9 | [protocol-parity.md](protocol-parity.md#services) | 40–60 |
| Security (ACLs, NAT, SSH, passwords, AAA, PPP authentication) | 8 | 78% | 6.2 | [protocol-parity.md](protocol-parity.md#security) | 25–40 |
| WAN, cabling and wireless | 6 | 62% | 3.7 | [hardware-parity.md](hardware-parity.md) | 60–90 |
| Simulation mode (event list, PDU details, filters, complex PDUs) | 8 | 70% | 5.6 | [ui-parity.md](ui-parity.md) | 22–35 |
| Workspace and windows (canvas, device tabs, physical view, drawing) | 10 | 63% | 6.3 | [ui-parity.md](ui-parity.md) | 40–60 |
| Teaching (activities, scoring, instructions, languages) | 8 | 80% | 6.4 | [gaps.md](gaps.md) | 30–50 |
| Device families and hardware (models, modules, ASA, IoT) | 5 | 25% | 1.3 | [hardware-parity.md](hardware-parity.md) | 80–120 |
| **Total** | **100** | | **73.8** | | **~550–880** |

## Audiences

- **CCNA 1 (Introduction to Networks):** basic switch and router configuration, passwords and
  SSH, IPv4 and IPv6 addressing, ARP and NDP, ping and traceroute, Simulation mode to watch
  frames. Almost all of it works; what is missing is console breadth and some Simulation-mode
  detail (per-layer PDU view layout, protocol frames such as BPDUs).
- **CCNA 2 (SRWE):** VLANs, trunks, inter-VLAN routing (router-on-a-stick and multilayer), STP,
  EtherChannel, DHCPv4, SLAAC, FHRP, port security, wireless LANs, static routing. Wireless and
  HSRP are the large missing pieces.
- **CCNA 3 (ENSA):** single-area OSPF, ACLs, NAT, WAN concepts, VPN, QoS, network management,
  automation. OSPF, ACLs, NAT, GRE/IPsec VPNs, SNMP, syslog and NTP work; QoS and the automation topics do not.
- **Instructor:** builds activities with checks, a score and a side-by-side comparison with the answer; no answer-network view.

## Stage

**Pre-alpha:** every core workflow works end to end (build a lab, configure by typing, ping,
watch frames, save), but there is no packaged release, no activities, and the console still
misses commands students meet in the first labs. Alpha gates are in
[`roadmap.md`](roadmap.md#alpha-gates).

## Better than the reference

- **Real frames:** every frame is encoded and decoded; the event list exports to pcap for
  Wireshark or any analyser.
- **Open lab format:** `.pcraft` is readable JSON ([`file-format.md`](file-format.md)).
- **Agent control:** every action is a command, reachable from the CLI and an MCP server.
- **Runs everywhere:** Linux, Windows, macOS, BSD and the browser, with no account or sign-in.
- **Drop reasons:** a dropped frame says why (ACL, VLAN not allowed, blocked by STP…).

## Revision history

| Date | Change | Summary |
|---|---|---|
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
