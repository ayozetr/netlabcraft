# Decisions

_Updated 2026-10-29._

- **2026-10-29 — Clean room, no reverse engineering.** PacketCraft is built from public standards and black-box observation only. The reference app's licence forbids reverse engineering, and the storytold Craft apps forbid reading vendor binaries; both rule out decompiling it or its file format.
- **2026-10-29 — No `.pkt` import for now.** Reading the reference app's encrypted, compressed lab format would mean either reverse engineering it or using third-party work derived from that. Out of scope until a lawyer-reviewed plan exists; labs use the open `.pcraft` format.
- **2026-10-29 — Console vocabulary, our own words.** Command names and configuration syntax are kept (they are what students learn and what configs are made of); prompts follow the usual `name>` / `name#` / `name(config)#` convention. All output text, table layouts and help strings are original.
- **2026-10-29 — Real frames, simulated time.** Devices exchange encoded frames over a discrete-event queue in microseconds. Deterministic, wasm-safe, and captures are valid pcap.
- **2026-10-29 — Routers drop the packet that starts ARP; hosts queue it.** Matches what learners see on real equipment (the first ping across a router times out).
- **2026-10-29 — Proxy ARP on by default on routers**, so a static route out of an Ethernet interface works, as on common router software.
- **2026-10-29 — Loop guard instead of silent hangs.** Until STP exists, a frame crossing 64 switches is dropped and the UI reports a switching loop.
- **2026-10-29 — Generic device kinds.** No vendor model names or pictures; ports are named with the usual `GigabitEthernet0/0` / `FastEthernet0/1` convention.
- **2026-10-29 — Same arrangement, own artwork.** Like WordCraft with Word, the window follows the reference app's layout and workflow (where things are, what tabs a device window has, the Realtime/Simulation switch, the PDU list), observed black-box. Icons, device pictures, colours' exact values and all wording are ours, drawn in code. Device models get generic names (`Router`, `Switch-24`, `PC`, `Server`).
