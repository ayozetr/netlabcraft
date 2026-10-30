# Roadmap

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (rewritten: milestones as built, current focus, alpha and beta gates) · **Target:** Cisco Packet Tracer 9.0

Milestones, the current focus and what comes next. The summary is in
[`ROADMAP.md`](../ROADMAP.md); the ranked work list is [gaps.md](gaps.md).

## Current focus

Make the first CCNA course work end to end without surprises: console coverage (gap 2) and
protocol frames in Simulation mode (gap 3), then activities (gap 1), which is what turns a
simulator into a teaching tool.

## Milestones

| Milestone | State | What it holds |
|---|---|---|
| **M0 — Foundations** | ✅ 2026-10-29 | Workspace and gates, wire formats, lab model and file format, discrete-event simulator, console, engine with undo and panic guard, egui app in the reference app's arrangement, CLI, MCP, web build. |
| **M1 — Services and security** | ✅ 2026-10-29 (in the first commit, with M0) | DHCP (server, relay, client), STP, switch VLAN interfaces, OSPF, ACLs, NAT/PAT, passwords and lines, Telnet/SSH, server HTTP/DNS/DHCP and the PC web browser, port security. |
| **M2 — Layer 2 depth, IPv6 and WAN** | ✅ 2026-10-29 | M2.1 EtherChannel and VTP · M2.2 IPv6 · M2.3 multilayer switch · M2.4 RIP · M2.5 serial WAN links. |
| **M3 — CCNA 1 complete** | next | Console coverage, CDP, protocol frames in Simulation mode, PDU details layout. |
| **M4 — Teaching** | planned | Activities with expected results and a score, instructions panel, translations (Spanish first). |
| **M5 — CCNA 2 complete** | planned | Wireless LANs, HSRP, DTP, RSTP timers, DHCPv6. |
| **M6 — CCNA 3 complete** | planned | OSPF depth, PPP authentication, ACL depth, network services, VPN basics. |
| **M7 — First release** | planned | Icon, installers, workflows, About window ([releasing.md](releasing.md)). |

## Alpha gates

PacketCraft is **alpha** when a student can do the CCNA 1 labs in it without falling back to
the reference app:

1. Every command of the CCNA 1 labs is accepted, with output in our own wording (gap 2): 10–20 h left (CDP, clock, flash and interface views done).
2. Simulation mode shows the frames those labs talk about (gap 3): BPDUs, OSPF hellos and RIP updates done; what is left is optional for CCNA 1.
3. A packaged build exists for Linux, Windows and macOS (gap 13): 14–24 h.

**Total to alpha: ~25–45 h** (estimated; items 1 and 2 run in parallel).

## Beta gates

1. A CCNA-style lab book (VLANs, inter-VLAN routing, DHCP, OSPF, ACLs, NAT, IPv6, WAN) runs end to
   end by hand and through MCP.
2. Activities with scoring (gap 1).
3. Spanish interface (gap 12).
4. Wireless LANs (gap 4).

**Total to beta: ~180–280 h** (estimated).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | major | Rewritten: milestones as they were built (M0–M2), next milestones by CCNA course, alpha and beta gates with estimates. |
| 2026-10-29 | major | Created. |
