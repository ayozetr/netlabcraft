# Roadmap

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (M9.7: four more labs, console depth second round) · earlier: minor (M9.4: seven troubleshooting labs) · earlier: minor (M7 first release done: packages and the release workflow), major (alpha gate 1 met through the lab book; every beta gate met; milestone states and estimates after the security, instructor, Simulation-mode, wireless, end-device, automation and routing rounds) · **Target:** Cisco Packet Tracer 9.0

Milestones, the current focus and what comes next. The summary is in
[`ROADMAP.md`](../ROADMAP.md); the ranked work list is [gaps.md](gaps.md).

## Current focus

The CCNA lab book runs end to end, by the engine and over MCP, and the leftovers of the
earlier rounds landed (security, instructor tools, Simulation mode, wireless, end devices,
automation, switching and routing). The focus now is depth where it is honestly thin: more labs
walked to find console gaps (gap 1), the remaining crypto stand-ins and certificates (gaps 2
and 3), and a cellular network for the mobile devices (gap 4).

## Milestones

| Milestone | State | What it holds |
|---|---|---|
| **M0 — Foundations** | ✅ 2026-10-29 | Workspace and gates, wire formats, lab model and file format, discrete-event simulator, console, engine with undo and panic guard, egui app in the reference app's arrangement, CLI, MCP, web build. |
| **M1 — Services and security** | ✅ 2026-10-29 (in the first commit, with M0) | DHCP (server, relay, client), STP, switch VLAN interfaces, OSPF, ACLs, NAT/PAT, passwords and lines, Telnet/SSH, server HTTP/DHCP/DNS and the PC web browser, port security. |
| **M2 — Layer 2 depth, IPv6 and WAN** | ✅ 2026-10-29 | M2.1 EtherChannel and VTP · M2.2 IPv6 · M2.3 multilayer switch · M2.4 RIP · M2.5 serial WAN links. |
| **M3 — CCNA 1 complete** | ✅ 2026-10-31 (console coverage, CDP, protocol frames, the PC prompt, complex PDUs, the OSI Model view, scenarios and the Simulation-mode leftovers landed; the lab book's four CCNA 1 labs run) | Console coverage, CDP, protocol frames in Simulation mode, PDU details layout. |
| **M4 — Teaching** | ✅ 2026-10-31 (activities, wizard, variables, the comparison with the answer, the Spanish interface, and M4.5–M4.9: answer network, assessment tree, initial network, instructions per language, connectivity tests, signed results, locks) | Activities with expected results and a score, instructions panel, translations (Spanish first). |
| **M5 — CCNA 2 complete** | ✅ 2026-10-31 (wireless with group keys, a real PEAP tunnel and central switching, HSRP, VRRP with timers and VRRPv3, GLBP, DTP, RSTP, loop guard, DHCPv6, switch security, CoS, VTP pruning, routing leftovers, and the end devices of M5.85–M5.88; the lab book's nine CCNA 2 labs run) | Wireless LANs, FHRP, DTP, STP, LAN security, DHCPv6, static routing. |
| **M6 — CCNA 3 complete** | ✅ 2026-10-31 (OSPF as taught, PPP authentication, ACL depth, network management, GRE/IPsec with IKEv1 and IKEv2 on a real Diffie-Hellman with real AES and rekeying, QoS, zone-based firewall, MTU, RESTCONF and NETCONF over TLS and SSH, a controller with discovery; the lab book's seven CCNA 3 labs run; DES, 3DES, AH and certificates left, in [gaps.md](gaps.md)) | OSPF depth, PPP authentication, ACL depth, network services, VPN basics, QoS, automation. |
| **M7 — First release** | **done** (icon, Linux user install, About window, CI, packaging and the release workflow) | Linux, Windows, macOS and web packages, SHA256SUMS and a draft release, built by `release.yml` and tested end to end; builds are not signed or notarized yet ([releasing.md](releasing.md)). |
| **M8 — CCNA lab book** | ✅ 2026-10-31 | M8.1 fixes the book turned up · M8.2 CCNA 1 labs and the runner · M8.3 CCNA 2 labs · M8.4 CCNA 3 labs and the MCP run ([labs/](labs/README.md)). |

## Alpha gates

NetLabCraft is **alpha** when a student can do the CCNA 1 labs in it without falling back to
the reference app:

1. Every command of the CCNA 1 labs is accepted, with output in our own wording: **met**. The
   lab book's four CCNA 1 labs (device basics and SSH, dual-stack addressing, VLSM, ARP, ICMP
   and DHCP in Simulation mode) run command by command as written, and the fixes they turned
   up landed (M8.1). The course's own activities can't be copied, so labs beyond the book may
   still turn up details ([gaps.md](gaps.md), item 1).
2. Simulation mode shows the frames those labs talk about: met (ARP, ICMP, DHCP, DNS, TCP,
   BPDUs, OSPF, RIP and more, with pcap export), with complex PDUs, PDU details in OSI layers,
   Constant Delay and a capture filter.
3. A packaged build exists for Linux, Windows and macOS: **met** (`release.yml` builds them; not signed yet).
   Until it is lifted, Linux users install from source with `packaging/linux/install.sh`, and
   everyone else builds with Cargo.

**Total to alpha:** gates 1 and 2 are met; every alpha gate is met.

## Beta gates

1. A CCNA-style lab book (VLANs, inter-VLAN routing, DHCP, OSPF, ACLs, NAT, IPv6, WAN, switch
   security, HSRP) runs end to end by hand and through MCP (gaps table, item 1): **met**.
   [docs/labs/](labs/README.md) holds 27 original labs (4 for CCNA 1, 13 for CCNA 2, 10 for
   CCNA 3; labs 21 to 27 are troubleshooting labs, added in M9.4); each one's blocks run as
   written in `crates/engine/tests/lab_book.rs`, and 11 of them over `netlabcraft-cli mcp` in `apps/netlabcraft-cli/tests/mcp_lab_book.rs`. The fixes they
   turned up landed with regression tests (M8.1).
2. Activities with scoring: met.
3. Spanish interface: met (every window, tab and page, checked by tests).
4. Wireless LANs: met (access points, home routers, WLC with lightweight APs, WPA2-PSK and
   Enterprise with group keys).

**Total to beta:** every gate is met; beta follows alpha, so it waits on the same packaging
hold.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | M9.7: four more labs (28 to 31) join the lab book (31 labs, 14 over MCP) and the console gaps the first round left are fixed; the gaps table's item 1 narrowed again. |
| 2026-11-13 | minor | M9.4: seven troubleshooting labs (21 to 27) join the lab book (27 labs, 11 over MCP); the gaps table's item 1 narrowed. |
| 2026-11-13 | minor | M7 done: the release workflow and packages for Linux, Windows, macOS and the web, tested end to end; the packaged-build gate is met. |
| 2026-10-31 | major | M3–M6 and M8 done; alpha gate 1 met through the lab book; totals to alpha and beta now wait only on the packaging hold; current focus on depth (more labs, crypto stand-ins, certificates, cellular). |
| 2026-10-30 | minor | Beta gate 1 met: 20-lab book in `docs/labs/`, run as written by the engine and over MCP. |
| 2026-10-30 | minor | Current focus and milestone states after Simulation-mode depth, automation, security, leftovers and device families landed; beta gate 1 (the lab book) in progress; grammar count 958. |
| 2026-10-30 | major | Gates cite the new ranked gaps table instead of old gap numbers; alpha gates 1 and 2 met but for a walk through the labs; packaging and M7 on hold by the owner's decision; beta gates 2–4 met; estimates redone (alpha ~5–10 h, beta ~45–80 h). |
| 2026-10-30 | minor | Milestone states brought up to date: M3–M7 in progress, with what is left in each. |
| 2026-10-30 | major | Rewritten: milestones as they were built (M0–M2), next milestones by CCNA course, alpha and beta gates with estimates. |
| 2026-10-29 | major | Created. |
