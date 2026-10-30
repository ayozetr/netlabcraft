# Roadmap

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (focus and milestone states after the depth rounds; the lab book in progress) · **Target:** Cisco Packet Tracer 9.0

Milestones, the current focus and what comes next. The summary is in
[`ROADMAP.md`](../ROADMAP.md); the ranked work list is [gaps.md](gaps.md).

## Current focus

Prove the CCNA courses work end to end: a CCNA-style lab book is being written and run by hand
and through MCP (gap 1, in progress), fixing what it turns up. Automation, security depth,
Simulation-mode depth, the switching, routing and wireless leftovers and the device families
landed; what is left of them (gaps 2–8) is smaller and follows the lab book.

## Milestones

| Milestone | State | What it holds |
|---|---|---|
| **M0 — Foundations** | ✅ 2026-10-29 | Workspace and gates, wire formats, lab model and file format, discrete-event simulator, console, engine with undo and panic guard, egui app in the reference app's arrangement, CLI, MCP, web build. |
| **M1 — Services and security** | ✅ 2026-10-29 (in the first commit, with M0) | DHCP (server, relay, client), STP, switch VLAN interfaces, OSPF, ACLs, NAT/PAT, passwords and lines, Telnet/SSH, server HTTP/DHCP/DNS and the PC web browser, port security. |
| **M2 — Layer 2 depth, IPv6 and WAN** | ✅ 2026-10-29 | M2.1 EtherChannel and VTP · M2.2 IPv6 · M2.3 multilayer switch · M2.4 RIP · M2.5 serial WAN links. |
| **M3 — CCNA 1 complete** | in progress (console coverage, CDP, protocol frames, the PC prompt, complex PDUs, the OSI Model view of PDU details and scenarios landed; walking the labs is left) | Console coverage, CDP, protocol frames in Simulation mode, PDU details layout. |
| **M4 — Teaching** | in progress (activities, wizard, variables, the comparison with the answer and the Spanish interface landed; an answer-network view is left) | Activities with expected results and a score, instructions panel, translations (Spanish first). |
| **M5 — CCNA 2 complete** | in progress (wireless with group keys, HSRP v1/v2 with tracking, VRRP, GLBP, DTP, RSTP with the proposal/agreement handshake, loop guard, DHCPv6, switch security with option 82, STP tuning and floating static routes landed; a real PEAP tunnel and walking the labs left) | Wireless LANs, FHRP, DTP, STP, LAN security, DHCPv6, static routing. |
| **M6 — CCNA 3 complete** | in progress (OSPF as taught, PPP authentication, ACL depth and editing, network management, GRE/IPsec with IKEv1 and IKEv2, QoS with trust and shaping, zone-based firewall, `ip mtu`/`adjust-mss`, RESTCONF, NETCONF, the Programming app and a controller landed; automation depth, a real Diffie-Hellman and walking the labs left) | OSPF depth, PPP authentication, ACL depth, network services, VPN basics, QoS, automation. |
| **M7 — First release** | **on hold** (icon, Linux user install and About window done) | The owner has decided there is no release for now, and GitHub Actions is not available for the release workflow. Installers and workflows are not planned until the hold is lifted ([releasing.md](releasing.md)). |

## Alpha gates

NetLabCraft is **alpha** when a student can do the CCNA 1 labs in it without falling back to
the reference app:

1. Every command of the CCNA 1 labs is accepted, with output in our own wording: essentially
   met (958 grammar entries; the CCNA 1 lab commands, the PC prompt and the console fixes
   landed). What is left is walking the labs command by command to confirm it: ~5–10 h.
2. Simulation mode shows the frames those labs talk about: met (ARP, ICMP, DHCP, DNS, TCP,
   BPDUs, OSPF, RIP and more, with pcap export), with complex PDUs and PDU details in OSI
   layers.
3. A packaged build exists for Linux, Windows and macOS: **on hold** by the owner's decision.
   Until it is lifted, Linux users install from source with `packaging/linux/install.sh`, and
   everyone else builds with Cargo.

**Total to alpha: ~5–10 h** (estimated) for gates 1 and 2; gate 3 is on hold and not estimated.

## Beta gates

1. A CCNA-style lab book (VLANs, inter-VLAN routing, DHCP, OSPF, ACLs, NAT, IPv6, WAN, switch
   security, HSRP) runs end to end by hand and through MCP (gaps table, item 1): in progress.
2. Activities with scoring: met.
3. Spanish interface: met (every window, tab and page, checked by tests).
4. Wireless LANs: met (access points, home routers, WLC with lightweight APs, WPA2-PSK and
   Enterprise with group keys).

**Total to beta: ~45–80 h** (estimated): 25–40 h for the lab book, and as much again for the
fixes it turns up.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Current focus and milestone states after Simulation-mode depth, automation, security, leftovers and device families landed; beta gate 1 (the lab book) in progress; grammar count 958. |
| 2026-10-30 | major | Gates cite the new ranked gaps table instead of old gap numbers; alpha gates 1 and 2 met but for a walk through the labs; packaging and M7 on hold by the owner's decision; beta gates 2–4 met; estimates redone (alpha ~5–10 h, beta ~45–80 h). |
| 2026-10-30 | minor | Milestone states brought up to date: M3–M7 in progress, with what is left in each. |
| 2026-10-30 | major | Rewritten: milestones as they were built (M0–M2), next milestones by CCNA course, alpha and beta gates with estimates. |
| 2026-10-29 | major | Created. |
