# Target-app parity: NetLabCraft vs Cisco Packet Tracer

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (M9.11: BoardScript messages in the interface language) · earlier: minor (M9.11: the smart things share a room with physics) · earlier: minor (M9.5: IKEv1 group 24 is a real Diffie-Hellman group) · also merged with M9.6 and M9.8 (results replay; Resize Shape and accessible devices) · earlier: minor (M9.5: AH next to ESP in one transform set is a real bundle), minor (M9.5: the IKE SA has a lifetime and is replaced before it runs out), minor (counts measured again after seven troubleshooting labs; percentages not re-scored) · also merged with M9.2 and M9.3 (routing timers; real DES, 3DES, AH and elliptic-curve Diffie-Hellman), minor (packaged-build gate met), major (the lab book run end to end, security leftovers, instructor tools, Simulation-mode and wireless leftovers, end devices, automation depth and routing leftovers: areas and course topics re-scored, the untested-labs deduction cut; counts measured again) · **Target:** Cisco Packet Tracer 9.0 (as used in CCNA courses)

This is the authoritative parity assessment. [`ROADMAP.md`](../ROADMAP.md) summarizes it,
[`gaps.md`](gaps.md) lists every shortfall, and the area checklists hold the detail:
[protocols and commands](protocol-parity.md), [UI](ui-parity.md), [hardware](hardware-parity.md),
[localization](localization-parity.md). The generated command list is
[`command-reference.md`](command-reference.md); the lab book that checks the course topics end
to end is in [`labs/`](labs/README.md).

## Summary

| Number | Value | Kind |
|---|---|---|
| Engine commands | **138** | measured: `c!(` entries in `crates/engine/src/catalog.rs` |
| Console grammar entries | **1015** across 37 modes | measured: `spec!(` rows in `crates/ios/src/grammar.rs`; modes are its `Modes` constants less `ANY_MODE`, `BOTH` and the device flags `ROUTER`, `SWITCH`, `FIREWALL`, `CARD` |
| Device models | **51** | measured: `MODELS` in `crates/model/src/models.rs` (the fallback model not counted; the 7 router cards are in `MODULES`) |
| Sample labs | **31** | measured: `SAMPLES` in `crates/engine/src/sample.rs` |
| Lab-book labs | **27** | measured: numbered labs in `docs/labs/` (4 CCNA 1, 13 CCNA 2, 10 CCNA 3; labs 21 to 27 are troubleshooting labs), all run by `crates/engine/tests/lab_book.rs`, 11 also over MCP by `apps/netlabcraft-cli/tests/mcp_lab_book.rs` |
| Tests | **620** | measured: sum of "passed" from `cargo test --workspace -q` |
| **Ready for real work** (weighted) | **~91%** | estimated; written weights below (90.9 computed) |
| **CCNA 1 student** (Introduction to Networks) | **~93%** | estimated; [method](#audiences) |
| **CCNA 2 student** (Switching, Routing and Wireless Essentials) | **~94%** | estimated; [method](#audiences) |
| **CCNA 3 student** (Enterprise Networking, Security and Automation) | **~90%** | estimated; [method](#audiences) |
| **Instructor** (builds graded activities) | **~90%** | estimated; activities with checks, points, sections, hints, values per student, a time limit and a password lock, plus an answer network, an assessment tree, an initial network, instructions per language, connectivity tests, signed results and locks |
| Stage | **pre-alpha** | see [Stage](#stage) |
| Remaining to alpha | **met** | every alpha gate is met, the packaged-build gate through `release.yml` ([roadmap.md](roadmap.md#alpha-gates)) |
| Remaining to ~95% in every area | **~55–110 Opus 5.5 agent-hours** | estimated; sum of the area rows below |

### Readiness by audience

| Audience | Ready % | Opus 5.5 agent wall-clock hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (the weighted total) | ~91% | ~55–110 (sum of the area rows) | Console depth beyond the lab book, workspace, device families (cellular), the remaining crypto stand-ins |
| CCNA 1 student | ~93% | ~2–5 | More labs walked; small console and host-app details they turn up |
| CCNA 2 student | ~94% | ~3–8 | More labs walked |
| CCNA 3 student | ~90% | ~8–16 | certificates, IKE SA rekeying, a controller closer to the course's, virtualization |
| Instructor | ~90% | ~4–8 | More interface languages |

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
| Device console (modes, commands, `show` output, help, completion) | 20 | 86% | 17.2 | [protocol-parity.md](protocol-parity.md#console) | 15–30 |
| Layer 2 (VLANs, trunks, STP, EtherChannel, VTP, port security, DTP, snooping, ARP inspection, CoS) | 12 | 97% | 11.64 | [protocol-parity.md](protocol-parity.md#layer-2) | 1–3 |
| Layer 3 (static, RIP, RIPng, OSPF, EIGRP, IPv6, multilayer switching, FHRP) | 15 | 96% | 14.4 | [protocol-parity.md](protocol-parity.md#layer-3) | 3–6 |
| Services (DHCP, DNS, HTTP, FTP, email, NTP, syslog, AAA, SNMP, RESTCONF, NETCONF, the controller) | 8 | 97% | 7.76 | [protocol-parity.md](protocol-parity.md#services) | 2–4 |
| Security (ACLs, NAT, SSH, passwords, AAA, PPP authentication, VPN, firewalls) | 8 | 94% | 7.52 | [protocol-parity.md](protocol-parity.md#security) | 4–8 |
| WAN, cabling and wireless | 6 | 93% | 5.58 | [hardware-parity.md](hardware-parity.md) | 4–8 |
| Simulation mode (event list, PDU details, filters, complex PDUs, scenarios) | 8 | 90% | 7.2 | [ui-parity.md](ui-parity.md) | 3–6 |
| Workspace and windows (canvas, device tabs, physical view, drawing) | 10 | 85% | 8.5 | [ui-parity.md](ui-parity.md) | 10–18 |
| Teaching (activities, scoring, instructions, languages, instructor tools) | 8 | 88% | 7.04 | [gaps.md](gaps.md) | 6–12 |
| Device families and hardware (models, modules, firewalls, phones, mobile devices, IoT, boards, WAN emulation) | 5 | 82% | 4.1 | [hardware-parity.md](hardware-parity.md) | 8–14 |
| **Total** | **100** | | **90.94** | | **~55–110** |

## Audiences

**Method.** Each course is split into its lab topics, weighted by their share of the course's
Packet Tracer activities (estimated from the public course outlines). Each topic is scored for
how much of what its labs do works here, depth included: a command that is accepted but not
simulated scores low. The weighted sum then loses points for labs nobody has run: **5 points**
while no lab book had been run end to end (beta gate 1), and **2 points** now that the
[lab book](labs/README.md) runs. The 2 points stay because the book is our own (27 labs, not
the course's own activities, which can't be copied), so the topics it doesn't exercise can
still hide surprises. Topics the book exercises are scored from what its labs showed; the
others from the features and tests alone. The earlier figures (CCNA 2 ~92%, CCNA 3 ~91% in the
first estimate) counted topics as present without weighing their depth or the untested labs.

| CCNA 1 topic | Weight | Ready |
|---|---|---|
| Device basics, passwords, SSH (lab 1) | 15 | 97% |
| IPv4 addressing and hosts (labs 2, 3) | 15 | 98% |
| IPv6 addressing, SLAAC, DHCPv6 (lab 2) | 10 | 94% |
| ARP, NDP, ping, traceroute (lab 4) | 10 | 96% |
| Switch basics, MAC table | 10 | 95% |
| Simulation mode and PDU details (complex PDUs, OSI view, scenarios, constant delay, capture filter; lab 4) | 15 | 96% |
| Host apps (prompt, browser, email, FTP, terminal) | 10 | 90% |
| Cabling and the physical layer | 10 | 90% |
| Application services (DNS, DHCP, HTTP) | 5 | 95% |
| **Weighted, less 2** | | **94.9 − 2 ≈ 93%** |

| CCNA 2 topic | Weight | Ready |
|---|---|---|
| VLANs, trunks, DTP (lab 5) | 15 | 97% |
| Inter-VLAN routing (router-on-a-stick, multilayer; lab 6) | 10 | 97% |
| STP and RSTP (PortFast, BPDU guard and filter, root and loop guard, cost, priority, proposal/agreement; lab 7) | 12 | 96% |
| EtherChannel (lab 8) | 8 | 97% |
| DHCPv4 and relay (lab 9) | 8 | 97% |
| SLAAC and DHCPv6 (lab 9) | 7 | 92% |
| FHRP (HSRP, VRRP with timers and tracking, GLBP; lab 10) | 7 | 97% |
| LAN security (port security, DHCP snooping with option 82, ARP inspection; lab 11) | 10 | 96% |
| Wireless LANs (WLC, WPA2, a real PEAP tunnel, central switching, group keys; lab 12) | 13 | 92% |
| Static and floating static routes, IPv4 and IPv6 (lab 13) | 10 | 97% |
| **Weighted, less 2** | | **95.8 − 2 ≈ 94%** |

| CCNA 3 topic | Weight | Ready |
|---|---|---|
| Single-area OSPFv2 (lab 14) | 18 | 97% |
| ACLs (lab 15) | 18 | 96% |
| NAT (lab 16) | 10 | 97% |
| Network management (CDP, LLDP, NTP, syslog, SNMP, backups, images, password recovery; lab 19) | 14 | 95% |
| VPN and IPsec (lab 17) | 8 | 94% |
| QoS (lab 18) | 5 | 86% |
| WAN concepts | 4 | 90% |
| Design and troubleshooting labs | 15 | 91% |
| Automation (REST APIs, controllers; lab 20) | 5 | 80% |
| Virtualization and SDN | 3 | 35% |
| **Weighted, less 2** | | **91.9 − 2 ≈ 90%** |

- **CCNA 1 (Introduction to Networks):** the four CCNA 1 labs of the book run as written, and
  Simulation mode has every option the course uses: Constant Delay, capturing only chosen
  protocols, absolute start times and the sender's own decisions in the OSI view. What is left
  is walking more labs and the host-app details they turn up.
- **CCNA 2 (SRWE):** thirteen labs of the book run, four of them troubleshooting labs (VLANs, inter-VLAN routing, STP, EtherChannel,
  DHCP, HSRP, switch security, a WLC, static routes). Wireless is no longer far behind:
  WPA2-Enterprise runs a real PEAP tunnel with EAP over RADIUS, and WLANs can be centrally
  switched; the PEAP key exchange is a toy size and there are no certificates. VRRP follows its
  timers now; GLBP hellos are still not on the wire (their format isn't public).
- **CCNA 3 (ENSA):** ten labs of the book run, three of them troubleshooting labs (OSPF, ACLs, NAT, GRE over IPsec, QoS, network
  management, RESTCONF). IPsec runs a real Diffie-Hellman exchange (MODP groups 1 to 16 and 24, and the P-256 and P-384 curves) and real AES, DES and
  3DES with SHA-1, SHA-256 or MD5 HMACs under IKEv1 and IKEv2, with lifetimes and child-SA
  rekeying, and AH on its own is real (tunnel mode); AH bundles with ESP (AH outside ESP) run for real; the IKE SA has a lifetime and is replaced before it runs out. Automation scores 80%:
  RESTCONF in JSON and XML over TLS 1.2, NETCONF with a candidate datastore over SSH 2.0, and a
  controller that discovers routers over CDP and LLDP and pushes templates; TLS has no
  certificate, the SSH host key is a stand-in, and the controller is still smaller than the
  course's network controller (no policies, no assurance pages). QoS gained 802.1p CoS.
- **Instructor:** the answer network, assessment tree, initial network, instructions per
  language, connectivity tests, signed results and locks landed. A JSON results
  file embeds the student's network and is replayed to check the claimed score, so a forged
  score fails (time claims and CSV files are not replayed), and the interface comes in two
  languages.

## Stage

**Pre-alpha:** every core workflow works end to end (build a lab, configure by typing, ping,
watch frames, grade an activity, save), and the 20-lab book runs as written, by the engine and
over MCP. Every alpha gate and every beta gate is met; packages are built by `release.yml`.
Gates are in [`roadmap.md`](roadmap.md#alpha-gates).

## Better than the reference

- **Real frames:** every frame is encoded and decoded; the event list exports to pcap for
  Wireshark or any analyser.
- **Open lab format:** `.nlcraft` is readable JSON ([`file-format.md`](file-format.md)).
- **Agent control:** every action is a command, reachable from the CLI and an MCP server.
- **Runs everywhere:** Linux, Windows, macOS, BSD and the browser, with no account or sign-in.
- **Drop reasons:** a dropped frame says why (ACL, VLAN not allowed, blocked by STP…).
- **Labs as scripts:** every lab in the lab book is also a test, so a change that breaks a
  course topic fails the build.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | BoardScript compile and run messages in Spanish (M9.11). The IoT and boards gap keeps one room per lab and BoardScript instead of Python or JavaScript. Scores unchanged. |
| 2026-11-13 | minor | The smart things share a room with physics (M9.11): a heater or air conditioner changes the thermostat's reading in simulated time (`iot.environment`, `iot-climate` sample); the IoT gap keeps BoardScript's English-only messages and the lack of rooms. Scores unchanged. |
| 2026-11-13 | minor | IKEv1 group 24 runs for real (M9.5); only a no-cipher transform set keeps a crypto stand-in; no percentage changed. |
| 2026-11-13 | minor | AH bundled with ESP in one transform set runs for real (M9.5); IKEv1 group 24 is what is left of the crypto stand-ins; no percentage changed. |
| 2026-11-13 | minor | IKE SA lifetimes and rekeying (M9.5); the crypto stand-ins left are AH bundled with ESP and IKEv1 group 24; no percentage changed. |
| 2026-11-13 | minor | Results replay (M9.6): the forgery gap is closed for JSON results; no percentage changed. |
| 2026-11-13 | minor | Seven troubleshooting labs (21 to 27) join the book: 27 labs, 11 over MCP. Counts measured again (1015 grammar entries, 138 commands, 620 tests); the percentages are not re-scored: the new labs found 12 small console gaps, fixed, and the estimate that more labs would turn up more is unchanged. |
| 2026-11-13 | minor | VRRPv3 router advertisements for IPv6 (M9.2): the routing leftovers (old gaps item 5) are done; no percentage changed. |
| 2026-11-13 | minor | EIGRP for IPv6 per-interface hello and hold timers (M9.2); the CCNA 2 student leftovers lose them; no percentage changed. |
| 2026-11-13 | minor | RIPng timers, hold-down and triggered updates (M9.2): the CCNA 2 student leftovers lose RIPng; no percentage changed (the remaining routing leftovers are 2–4 h in gaps.md). |
| 2026-11-13 | minor | Groups 19 and 20 (elliptic-curve Diffie-Hellman) run for real; IKE SA rekeying is what is left of the crypto stand-ins. |
| 2026-11-13 | minor | DES, 3DES and AH run for real (IKEv1, IKEv2, ESP, AH in tunnel mode); the CCNA 3 notes say the elliptic-curve groups and IKE SA rekeying are what is left of the crypto stand-ins. |
| 2026-10-31 | major | The lab book (20 labs, run end to end and 8 over MCP), real Diffie-Hellman, AES-192/256, SHA-256, IKEv1 ciphers and rekeying, default MTU, more inspections; instructor tools; Simulation-mode leftovers; a real PEAP tunnel and central switching; tablets, smartphones, ten Wi-Fi IoT things, programmable boards, the phone's settings, power supplies; XML, IPv6, TLS and SSH in automation, NETCONF candidate and locks, controller discovery; CoS, VRRP timers and VRRPv3, EIGRP for IPv6 on the wire, RIP timers and RIPng, VTP pruning. Console 84% → 86%, layer 2 95% → 97%, layer 3 94% → 96%, services 95% → 97%, security 90% → 94%, WAN/cabling/wireless 90% → 93%, Simulation mode 85% → 90%, workspace 83% → 85%, teaching 80% → 88%, device families 70% → 82%; total 87.5 → 90.94; hours to ~95% ~90–165 → ~55–110. Method: the untested-labs deduction goes from 5 to 2 points now that the lab book runs. CCNA 1 ~88% → ~93%, CCNA 2 ~88% → ~94%, CCNA 3 ~84% → ~90% (automation 60% → 80%, VPN 88% → 94%, wireless 85% → 92%), instructor ~78% → ~90%. Counts measured again (138 commands, 1003 grammar entries across 37 modes, 51 models, 31 samples, 20 lab-book labs, 591 tests). |
| 2026-10-30 | major | Complex PDUs, OSI view and scenarios; RESTCONF, NETCONF, the Programming app and a controller; zone-based firewall, `ip inspect`, `ip mtu`, `adjust-mss`, IKEv2 and real ESP; switch, QoS, routing and wireless leftovers; switch card, PoE, SIP phones, IoT things and gateway, cloud and modems. Console 82% → 84%, layer 2 90% → 95%, layer 3 92% → 94%, services 94% → 95%, security 86% → 90%, WAN/cabling/wireless 87% → 90%, Simulation mode 70% → 85%, workspace 80% → 83%, device families 60% → 70%; total 83.6 → 87.5; hours to ~95% ~150–250 → ~90–165. Course topics re-scored with the same method: CCNA 1 ~86% → ~88%, CCNA 2 ~85% → ~88%, CCNA 3 ~79% → ~84% (automation 15% → 60%, QoS 60% → 82%, VPN 75% → 88%). Counts measured again (114 commands, 958 grammar entries across 37 modes, 37 models, 28 samples, 443 tests). |
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
