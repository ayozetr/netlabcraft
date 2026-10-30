# Decisions

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (renamed to NetLabCraft)

Choices that shape NetLabCraft, newest last within each date, with the reason for each.

- **2026-10-29 — Clean room, no reverse engineering.** NetLabCraft is built from public standards and black-box observation only. The reference app's licence forbids reverse engineering, and the storytold Craft apps forbid reading vendor binaries; both rule out decompiling it or its file format.
- **2026-10-29 — No `.pkt` import for now.** Reading the reference app's encrypted, compressed lab format would mean either reverse engineering it or using third-party work derived from that. Out of scope until a lawyer-reviewed plan exists; labs use the open `.nlcraft` format.
- **2026-10-29 — Console vocabulary, our own words.** Command names and configuration syntax are kept (they are what students learn and what configs are made of); prompts follow the usual `name>` / `name#` / `name(config)#` convention. All output text, table layouts and help strings are original.
- **2026-10-29 — Real frames, simulated time.** Devices exchange encoded frames over a discrete-event queue in microseconds. Deterministic, wasm-safe, and captures are valid pcap.
- **2026-10-29 — Routers drop the packet that starts ARP; hosts queue it.** Matches what learners see on real equipment (the first ping across a router times out).
- **2026-10-29 — Proxy ARP on by default on routers**, so a static route out of an Ethernet interface works, as on common router software.
- **2026-10-29 — Loop guard instead of silent hangs.** A frame crossing 64 switches is dropped and the UI reports a switching loop (spanning tree normally prevents it).
- **2026-10-29 — Generic device kinds.** No vendor model names or pictures; ports are named with the usual `GigabitEthernet0/0` / `FastEthernet0/1` / `Serial0/0/0` convention.
- **2026-10-30 — Model names describe their ports.** The workspace shows names such as `Router 3GE` or `Switch 24FE`; lab files keep the stable ids (`Router-3`, `Switch-24`) and commands accept both. Vendor model names appear only in [model-equivalents.md](model-equivalents.md), to tell students which model to place for a course lab, never as names of NetLabCraft models.
- **2026-10-29 — Same arrangement, own artwork.** Like WordCraft with Word, the window follows the reference app's layout and workflow (where things are, what tabs a device window has, the Realtime/Simulation switch, the PDU list), observed black-box. Icons, device pictures, colours' exact values and all wording are ours, drawn in code.
- **2026-10-29 — One click opens a device**, as in the reference app (a double click was the first version).
- **2026-10-29 — Computed protocols.** Spanning tree, OSPF, RIP, EtherChannel negotiation and VTP are computed from the topology the way they converge, instead of exchanging timed hellos. Labs are correct at once and deterministic; the cost is that their frames are not in the event list yet ([gaps.md](gaps.md), gap 3).
- **2026-10-29 — Switches flush learned addresses on a topology change**, as a topology-change notification makes real switches do, so no address points at a path that is gone.
- **2026-10-29 — The VLAN database and VTP settings survive a reload**, as they live apart from the configuration on real switches. A VTP client's VLANs are not written into its running-config.
- **2026-10-29 — IPv6 neighbour discovery holds the packet**, on routers too, so the first IPv6 ping isn't lost (unlike IPv4 ARP on routers).
- **2026-10-29 — Both RIP versions behave classless.** `network` statements are classful, as typed, but subnets are advertised as they are; automatic summarisation is stored and not simulated.
- **2026-10-29 — One copper cable, auto-MDIX.** Superseded on 2026-10-30 (cable types below).
- **2026-10-30 — Cable types.** Links carry a kind: automatic (resolves to the right kind for the two ports, today's behaviour and the default in old files), copper straight-through, copper cross-over, fiber, console, serial DCE, serial DTE. Ports have a medium (copper, fiber, combo, serial, wireless, console, RS-232) and a cable only joins ports of its medium. Routers and switches do auto-MDIX unless `no mdix auto`; hosts and hubs don't, so a wrong copper cable leaves the link down and says why. Serial DTE puts the DCE at the far end. Console cables join a PC's RS-232 port to a device's Console port and are not network links. Copper reaches 100 m, fiber 2000 m.
- **2026-10-30 — Renamed to NetLabCraft.** The earlier name belongs to an unrelated company with a registered trademark in software services, and names built on "Packet" or "Tracer" lean on the reference app's name. Labs are `.nlcraft` with `"format": "netlabcraft"`; no files with the earlier name were ever shared, so none are read.
- **2026-10-29 — Serial frames are captured as Ethernet II.** The wire carries HDLC or PPP; the event list and pcap show the equivalent Ethernet II frame, marked with its framing, because a pcap file has one link type.
- **2026-10-29 — Multilayer switching is a model, not a mode.** `ip routing` and routed ports are only accepted on the Switch-ML-24 model; other switches say why.
- **2026-10-30 — Control frames show what the computed protocols say.** BPDUs (every 2 s), OSPF hellos (10 s) and RIP updates (30 s) go on the wire in their standard formats so Simulation mode and captures have them; receivers take them in quietly and the protocols stay computed. CDP frames are left out because their format isn't published as a standard.
- **2026-10-30 — Spanning tree takes its time.** Trees are still computed at once, but a port that starts forwarding waits 30 s (listening, learning) unless portfast or rapid-pvst, as students see on real equipment. Sample labs start converged.
- **2026-10-30 — Documentation follows the shared Craft format**: every page carries a review line and a revision history; parity is assessed with written weights and labelled estimates.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Renamed to NetLabCraft. |
| 2026-10-30 | minor | Cable types replace the single copper cable. |
| 2026-10-30 | minor | Model names describe their ports; vendor models only in the equivalents page. |
| 2026-10-30 | minor | Decisions from M1–M2 recorded; shared format. |
| 2026-10-29 | major | Created. |
