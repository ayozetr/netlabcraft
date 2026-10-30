# Decisions

## Clean room, no reverse engineering

NetLabCraft is built from public standards and black-box observation only. The reference app's licence forbids reverse engineering, and the storytold Craft apps forbid reading vendor binaries; both rule out decompiling it or its file format.

## No `.pkt` import for now

Reading the reference app's encrypted, compressed lab format would mean either reverse engineering it or using third-party work derived from that. Out of scope until a lawyer-reviewed plan exists; labs use the open `.nlcraft` format.

## Console vocabulary, our own words

Command names and configuration syntax are kept (they are what students learn and what configs are made of); prompts follow the usual `name>` / `name#` / `name(config)#` convention. All output text, table layouts and help strings are original.

## Real frames, simulated time

Devices exchange encoded frames over a discrete-event queue in microseconds. Deterministic, wasm-safe, and captures are valid pcap.

## Routers drop the packet that starts ARP; hosts queue it

Matches what learners see on real equipment (the first ping across a router times out).

## Proxy ARP on by default on routers

Proxy ARP on by default on routers, so a static route out of an Ethernet interface works, as on common router software.

## Loop guard instead of silent hangs

A frame crossing 64 switches is dropped and the UI reports a switching loop (spanning tree normally prevents it).

## Generic device kinds

No vendor model names or pictures; ports are named with the usual `GigabitEthernet0/0` / `FastEthernet0/1` / `Serial0/0/0` convention.

## Model names describe their ports

The workspace shows names such as `Router 3GE` or `Switch 24FE`; lab files keep the stable ids (`Router-3`, `Switch-24`) and commands accept both. Vendor model names appear only in [model-equivalents.md](model-equivalents.md), to tell students which model to place for a course lab, never as names of NetLabCraft models.

## Same arrangement, own artwork

Like WordCraft with Word, the window follows the reference app's layout and workflow (where things are, what tabs a device window has, the Realtime/Simulation switch, the PDU list), observed black-box. Icons, device pictures, colours' exact values and all wording are ours, drawn in code.

## One click opens a device

One click opens a device, as in the reference app (a double click was the first version).

## Computed protocols

Spanning tree, OSPF, RIP, EtherChannel negotiation and VTP are computed from the topology the way they converge, instead of exchanging timed hellos. Labs are correct at once and deterministic. Their frames (BPDUs, hellos, updates, LACPDUs) are still sent at the usual intervals so Simulation mode and captures show them, but they report what the computation decided rather than drive it.

## Switches flush learned addresses on a topology change

Switches flush learned addresses on a topology change, as a topology-change notification makes real switches do, so no address points at a path that is gone.

## The VLAN database and VTP settings survive a reload

The VLAN database and VTP settings survive a reload, as they live apart from the configuration on real switches. A VTP client's VLANs are not written into its running-config.

## IPv6 neighbour discovery holds the packet

IPv6 neighbour discovery holds the packet, on routers too, so the first IPv6 ping isn't lost (unlike IPv4 ARP on routers).

## Both RIP versions behave classless

`network` statements are classful, as typed, but subnets are advertised as they are; automatic summarisation is stored and not simulated.

## Cable types

Links carry a kind: automatic (resolves to the right kind for the two ports, today's behaviour and the default in old files), copper straight-through, copper cross-over, fiber, console, serial DCE, serial DTE. Ports have a medium (copper, fiber, combo, serial, wireless, console, RS-232) and a cable only joins ports of its medium. Routers and switches do auto-MDIX unless `no mdix auto`; hosts and hubs don't, so a wrong copper cable leaves the link down and says why. Serial DTE puts the DCE at the far end. Console cables join a PC's RS-232 port to a device's Console port and are not network links. Copper reaches 100 m, fiber 2000 m.

## Renamed to NetLabCraft

The earlier name belongs to an unrelated company with a registered trademark in software services, and names built on "Packet" or "Tracer" lean on the reference app's name. Labs are `.nlcraft` with `"format": "netlabcraft"`; no files with the earlier name were ever shared, so none are read.

## Serial frames are captured as Ethernet II

The wire carries HDLC or PPP; the event list and pcap show the equivalent Ethernet II frame, marked with its framing, because a pcap file has one link type.

## Multilayer switching is a model, not a mode

`ip routing` and routed ports are only accepted on the Switch-ML-24 model; other switches say why.

## Control frames show what the computed protocols say

BPDUs (every 2 s), OSPF hellos (10 s) and RIP updates (30 s) go on the wire in their standard formats so Simulation mode and captures have them; receivers take them in quietly and the protocols stay computed. CDP frames are left out because their format isn't published as a standard.

## Spanning tree takes its time

Trees are still computed at once, but a port that starts forwarding waits 30 s (listening, learning) unless portfast or rapid-pvst, as students see on real equipment. Sample labs start converged.

## Documentation follows the shared Craft format

Every page carries a review line and a revision history; parity is assessed with written weights and labelled estimates.

## No release for now

The owner has put releases on hold: no installers or published builds for now, and GitHub Actions is not available to run release workflows. The quality gates run locally with `cargo xtask ci`, Linux users can install from source with `packaging/linux/install.sh`, and the release plan in [releasing.md](releasing.md) waits until the hold is lifted. The alpha gate that asks for a packaged build waits with it.

## Powered devices start without an adapter

IP phones and lightweight APs placed in a lab have no power adapter plugged in: they boot only on a PoE switch port within its budget, or once their adapter is plugged in from the Physical tab. That is how students meet power over Ethernet in the course labs, and a dark device says why it has no power. Labs saved before power existed have no `adapter` field, and their phones and APs count as plugged in, so they open and work as before.

## SIP phones are known by their MAC address

A SIP phone registers with its MAC address as the user part, and the router finds the `voice register pool` whose `id mac` matches it. The router names the phone's number in the `P-Associated-URI` header of its `200 OK` (RFC 3455), so the phone learns its number (the pool's `number 1 dn N`) from the registration; nobody types a number into the phone.

## IoT timers run in simulated time

Things report on their interval only as simulated time advances (`sim.advance`, the play controls, waiting for a ping), never by the wall clock, so runs stay deterministic and a paused lab stays still. A thing also reports at once when its reading changes, so rules react without waiting for the timer.

## Pings don't set "don't fragment" unless asked

Host and router pings go out without DF, as on common operating systems and router software; `ping -f` on hosts and the `df-bit` option of a router's extended ping set it. Echo replies carry no DF, and GRE copies DF from the inner packet. With `ip mtu` in the path, a plain ping is fragmented and a DF ping gets "fragmentation needed", which is what MTU labs show.

## The IKEv2 Diffie-Hellman exchange stays a stand-in

IKEv2 messages follow RFC 7296, and with an `esp-aes` and `esp-sha-hmac` transform set the SK payloads and ESP are really encrypted with AES-128-CBC and HMAC-SHA1-96. The Diffie-Hellman value, though, is filler of the group's size, and both ends derive the shared secret from the two public values, so someone who saw the exchange could work out the keys. Real modular exponentiation over the course's groups would mean a big-number library or our own, for no change in what students see; it stays a documented stand-in, and nothing here is meant to protect real traffic.
