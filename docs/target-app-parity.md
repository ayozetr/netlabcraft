# Target-app parity: PacketCraft vs Cisco Packet Tracer

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (rewritten in the shared parity format: method, weights, audiences, estimates) · **Target:** Cisco Packet Tracer 9.0 (as used in CCNA courses)

This is the authoritative parity assessment. [`ROADMAP.md`](../ROADMAP.md) summarizes it,
[`gaps.md`](gaps.md) lists every shortfall, and the area checklists hold the detail:
[protocols and commands](protocol-parity.md), [UI](ui-parity.md), [hardware](hardware-parity.md),
[localization](localization-parity.md). The generated command list is
[`command-reference.md`](command-reference.md).

## Summary

| Number | Value | Kind |
|---|---|---|
| Engine commands | **40** | measured: `crates/engine/src/catalog.rs` |
| Console grammar entries | **225** across 12 modes | measured: `spec!` rows in `crates/ios/src/grammar.rs` |
| Device models | **13** | measured: `crates/model/src/models.rs` |
| Sample labs | **17** | measured: `crates/engine/src/sample.rs` |
| Tests | **129** | measured: `cargo test --workspace` |
| **Ready for real work** (weighted) | **~50%** | estimated; written weights below (58.2 computed) |
| **CCNA 1 student** (Introduction to Networks) | **~74%** | estimated; [method](#audiences) |
| **CCNA 2 student** (Switching, Routing and Wireless Essentials) | **~62%** | estimated; [method](#audiences) |
| **CCNA 3 student** (Enterprise Networking, Security and Automation) | **~52%** | estimated; [method](#audiences) |
| **Instructor** (builds graded activities) | **~45%** | estimated; activities with checks, points, hidden answers and a wizard; no timer, locking or variables |
| Stage | **pre-alpha** | see [Stage](#stage) |
| Remaining to alpha | **~25–45 Opus 5.5 agent-hours** | estimated; [roadmap.md](roadmap.md#alpha-gates) |
| Remaining to full parity | **~600–950 Opus 5.5 agent-hours** | estimated; sum of the rows below |

### Readiness by audience

| Audience | Ready % | Opus 5.5 agent wall-clock hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (ready for real work) | ~50% | ~600–950 | Activities and scoring, wireless, more device families (ASA, IoT, modules), services (FTP, email, AAA, syslog), physical workspace, translations |
| CCNA 1 student | ~70% | ~40–70 | Protocol frames in Simulation mode (BPDU, hellos), console polish (`show` coverage), PDU details layout, a few host apps (text editor, terminal) |
| CCNA 2 student | ~58% | ~110–170 | Wireless LANs (40–60 h), DTP and RSTP timers (10–16 h), DHCPv6 and SLAAC flags (8–12 h), HSRP (12–18 h), console polish |
| CCNA 3 student | ~45% | ~180–280 | OSPF multi-area and OSPFv3 (20–30 h), ACL depth and IPv6 ACLs (10–16 h), PPP authentication (6–10 h), VPN/GRE (15–25 h), QoS basics, SNMP/syslog/NTP, automation APIs |
| Instructor | ~45% | ~50–90 | Activity depth: timer, locking, variables, per-check feedback (30–50 h), translations |

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
| Device console (modes, commands, `show` output, help, completion) | 20 | 65% | 13.0 | [protocol-parity.md](protocol-parity.md#console) | 60–90 |
| Layer 2 (VLANs, trunks, STP, EtherChannel, VTP, port security, DTP) | 12 | 80% | 9.6 | [protocol-parity.md](protocol-parity.md#layer-2) | 25–40 |
| Layer 3 (static, RIP, OSPF, EIGRP, IPv6, multilayer switching) | 15 | 60% | 9.0 | [protocol-parity.md](protocol-parity.md#layer-3) | 70–110 |
| Services (DHCP, DNS, HTTP, FTP, email, NTP, syslog, AAA) | 8 | 40% | 3.2 | [protocol-parity.md](protocol-parity.md#services) | 40–60 |
| Security (ACLs, NAT, SSH, passwords, AAA, PPP authentication) | 8 | 72% | 5.8 | [protocol-parity.md](protocol-parity.md#security) | 35–55 |
| WAN, cabling and wireless | 6 | 35% | 2.1 | [hardware-parity.md](hardware-parity.md) | 60–90 |
| Simulation mode (event list, PDU details, filters, complex PDUs) | 8 | 65% | 5.2 | [ui-parity.md](ui-parity.md) | 22–35 |
| Workspace and windows (canvas, device tabs, physical view, drawing) | 10 | 55% | 5.5 | [ui-parity.md](ui-parity.md) | 60–90 |
| Teaching (activities, scoring, instructions, languages) | 8 | 45% | 3.6 | [gaps.md](gaps.md) | 55–90 |
| Device families and hardware (models, modules, ASA, IoT) | 5 | 25% | 1.3 | [hardware-parity.md](hardware-parity.md) | 80–120 |
| **Total** | **100** | | **58.2** | | **~600–950** |

## Audiences

- **CCNA 1 (Introduction to Networks):** basic switch and router configuration, passwords and
  SSH, IPv4 and IPv6 addressing, ARP and NDP, ping and traceroute, Simulation mode to watch
  frames. Almost all of it works; what is missing is console breadth and some Simulation-mode
  detail (per-layer PDU view layout, protocol frames such as BPDUs).
- **CCNA 2 (SRWE):** VLANs, trunks, inter-VLAN routing (router-on-a-stick and multilayer), STP,
  EtherChannel, DHCPv4, SLAAC, FHRP, port security, wireless LANs, static routing. Wireless and
  HSRP are the large missing pieces.
- **CCNA 3 (ENSA):** single-area OSPF, ACLs, NAT, WAN concepts, VPN, QoS, network management,
  automation. OSPF, ACLs and NAT work; VPN, QoS, SNMP/syslog/NTP and the automation topics do not.
- **Instructor:** builds activities with an answer network and a score. Not started.

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
