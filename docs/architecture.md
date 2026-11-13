# Architecture

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (M9.10: the cellular network: modules in every layer, derived `cell_links`) · earlier: minor (M9.5: IKEv1 group 24 is a real Diffie-Hellman group) · earlier: minor (M9.5: AH next to ESP in one transform set is a real bundle), minor (M9.5: the IKE SA has a lifetime and is replaced before it runs out), minor (eleven lab-book labs over MCP) · also merged with M9.2 and M9.3 (routing timers; real DES, 3DES, AH and elliptic-curve Diffie-Hellman), minor (one modular-exponentiation engine), minor (modules for Diffie-Hellman, AES-256, SHA-256, TLS, SSH, PEAP, RIPng, IKEv1 keys, central switching, boards and BoardScript, power supplies, Simulation-mode options, instructor tools and the lab book; the duplicated modular exponentiation noted) · also merged with M9.11 (minor (M9.11: BoardScript messages in the interface language))

How NetLabCraft is built. Rules for contributors and agents are in [`AGENTS.md`](../AGENTS.md);
the reasons behind the choices are in [decisions.md](decisions.md).

## Layers

`cargo xtask layers` enforces that each crate depends only on lower layers, and that nothing below
L6 depends on a UI toolkit.

| Layer | Crate | Role |
|---|---|---|
| L0 | `netlabcraft-core` | Value types: `MacAddr`, `Ipv4Net`, `Ipv6Net` (with EUI-64 and link-local helpers), `VlanId`, `VlanSet`, `DeviceId`. |
| L1 | `netlabcraft-wire` | Wire formats and checksums: Ethernet II/802.1Q, ARP, IPv4, ICMP, UDP, TCP, DHCP, DNS (`dns.rs`), IPv6/ICMPv6/NDP (`ipv6.rs`), HDLC and PPP framing (`serial.rs`), TCP (`tcp.rs`), DHCPv6, OSPF and OSPFv3 packets and LSAs, EIGRP, LACP, LLDP, RADIUS, TACACS+, SNMP, IKE and ESP (`ike.rs`, `vpn.rs`, with real AES-, DES- and 3DES-CBC ESP and truncated HMACs), DES and 3DES (`des.rs`) the CBC cipher wrapper IKE and ESP use (`cipher.rs`) and the Authentication Header (`ah.rs`, tunnel mode and the transport mode an AH-outside-ESP bundle uses), IKEv2 (`ikev2.rs`), Diffie-Hellman over the MODP groups 1, 2, 5, 14, 15 and 16 (built from pi) and 24 (RFC 5114 constants, subgroup-checked) with its own big integers, Montgomery multiplication and windowed exponentiation (`dh.rs`), elliptic-curve Diffie-Hellman over P-256 and P-384, groups 19 and 20 (`ec.rs`, on the same big integers), the keyed algorithms IKE and ESP negotiate (`suite.rs`: PRFs, prf+, HMAC-MD5-96, HMAC-SHA1-96, HMAC-SHA-256-128), TLS 1.2 with the anonymous DH suite (`tls.rs`) and SSH 2.0 with `diffie-hellman-group14-sha256` (`ssh.rs`), both over the 2048-bit group of `modp.rs` (a thin layer over `dh.rs`), the PEAP tunnel (`peap.rs`), RIPng (`ripng.rs`), VRRP v2/v3 (`vrrp.rs`), SIP with SDP (`sip.rs`), HTTP/1.1 with Basic login and percent-encoding (`http.rs`), NETCONF framing (`netconf.rs`), a small strict XML reader and writer (`xml.rs`, no DOCTYPE, size and depth caps), IPv4 fragmentation, reassembly and MSS clamping (`ipfrag.rs`), CAPWAP and 802.11 (`wlan.rs`, with the AES key wrap of group keys), the MD5, SHA-1, SHA-256 (`sha256.rs`) and AES code those need (`aes.rs` has the inverse cipher, CBC and key wrap; `aes256.rs` the 192- and 256-bit key schedules), the attach exchange of the cellular network (`cell.rs`), the pcap writer, and the one-line summaries the event list shows. Depends on `core` only. |
| L2 | `netlabcraft-model` | The lab document: devices, ports and their configuration, cables (a serial cable's first end is the DCE), the device catalogue (`models.rs`), access lists (`acl.rs`), server services (`services.rs`), interface names (`ifname.rs`), routing settings and loopbacks (`routing.rs`), switch security and switch QoS trust (`l2sec.rs`), management settings and flash (`mgmt.rs`), VPN with IKEv2 (`vpn.rs`), QoS, firewall, wireless, voice and SIP, IoT things and rules, cables (coaxial and phone too), modules and the physical view (`qos.rs`, `firewall.rs`, `wlan.rs`, `voice.rs`, `iot.rs`, `cable.rs`, `hardware.rs`, `physical.rs`), VRRP and GLBP (`fhrp.rs`), the zone-based firewall, inspection, IP MTU and MSS (`security.rs`), automation settings and the controller's routers (`automation.rs`), power over Ethernet (`poe.rs`), clouds (`cloud.rs`), the cellular network: coverage, sectors, admission and addresses worked out from the lab (`cellular.rs`), scenarios and their PDUs (`scenario.rs`), programmable boards and their IoT custom cables (`board.rs`), power-supply bays (`psu.rs`), activities with their sealed answer network, initial network, translations, weights and locks (`activity.rs`), the `.nlcraft` format (`file.rs`). |
| L3 | `netlabcraft-sim` | The discrete-event simulator (`lib.rs`) and the protocols computed from the topology: spanning tree with the RSTP handshake (`stp.rs`), EtherChannel bundles (`channel.rs`), OSPF (`ospf.rs`, with its database in `ospf_lsdb.rs`), OSPFv3 (`ospf6.rs`), RIP with its timers (`rip.rs`), RIPng (`rip6.rs`), EIGRP (`eigrp.rs`) and EIGRP for IPv6 (`eigrp6.rs`), HSRP (`hsrp.rs`), VRRP and GLBP (`fhrp.rs`), VTP (`vtp.rs`); IPv6 forwarding, NDP and autoconfiguration (`v6.rs`); DHCP snooping with option 82, ARP inspection and edge protection (`l2sec.rs`); loopbacks (`loopback.rs`); the DHCP server (`dhcpsrv.rs`), logging levels and timestamps (`logcfg.rs`), NTP, syslog and SNMP (`mgmt.rs`, `snmp.rs`); the zone-based firewall and classic inspection (`zfw.rs`), IP MTU with the 1500-byte default and path MTU discovery (`mtu.rs`), IKEv1 keys and ciphers (`ikev1.rs`), IKEv2 with child-SA and IKE SA rekeying (`ikev2.rs`); SIP phones and call control (`sip.rs`); RESTCONF and NETCONF transports over TLS and SSH, with client connections kept open (`automation.rs`); PEAP logins end to end through the controller and RADIUS (`peap.rs`); central switching in CAPWAP data (`central.rs`); cell towers, the attach exchange and what a device learns from it (`cell.rs`); BoardScript, compiled to steps with a step cap (`script.rs`), and the boards that run it (`board.rs`); Constant Delay, the capture filter and the sender's own decisions (`simmode.rs`); user PDUs, simple and complex (`pdu.rs`); the decisions recorded per event and the OSI view built from them (`explain.rs`); email, FTP, VPN, QoS with shaping, firewall, wireless, voice and IoT with timed reports in their own modules. |
| L4 | `netlabcraft-ios` | Device consoles: the command grammar (`grammar.rs`), dispatch and modes (`exec.rs`), `show` output and running-config (`render.rs`), the PC/server shell (`host.rs`), output filters (`pipe.rs`); `exec/` holds the dispatch for routing, EIGRP for IPv6 (`eigrp6.rs`), VRRP and GLBP (`fhrp.rs`), switch security and QoS trust, management, VPN, IKEv2 (`ikev2.rs`), QoS, firewall, router security (`security.rs`: zones, inspection, MTU, MSS), automation (`automation.rs`), wireless, voice, the device families (`families.rs`: PoE, the switch card, SIP) the switching and routing leftovers (`swroute.rs`: CoS, EIGRP summaries, RIP timers, RIPng, VTP pruning) and the central office server's cellular commands (`cellular.rs`). |
| L5 | `netlabcraft-engine` | `Session` (lab + simulator + consoles + undo), the command catalog (`catalog.rs`, `cmd.rs`), sample labs (`sample.rs`); RESTCONF data and edits (`automation.rs`), NETCONF operations (`netconf.rs`) and the controller (`controller.rs`), all applying changes as console lines through the grammar; scenarios and PDUs (`pdu.rs`); power adapters and sources (`families.rs`); programmable boards (`board.rs`); the cellular commands (`cellular.rs`); instructor tools (`instructor.rs`: sealing the answer network, the assessment tree, the initial network, translations, connectivity tests, signed results, locks the engine enforces). The lab book runs through the engine in `tests/lab_book.rs`, whose runner (`tests/labbook/`) reads each `docs/labs/*.md` file's `lab`, `console` and `verify` blocks. |
| L6 | `netlabcraft-ui-egui` | The egui interface. |
| L6 | `netlabcraft-mcp` | MCP server over stdio; `apps/netlabcraft-cli/tests/mcp_lab_book.rs` runs eleven lab-book labs through it over JSON-RPC. |
| L7 | `apps/netlabcraft`, `apps/netlabcraft-cli`, `apps/netlabcraft-web` | Shells: desktop window and file dialogs, headless CLI, browser. |

## The simulator

- **Time** is simulated, in microseconds. Nothing reads the wall clock (it traps on wasm), so runs
  are deterministic.
- **Configuration lives in the model; the simulator keeps only dynamic state** (ARP and neighbour
  caches, MAC tables, packets waiting for an address, leases, NAT translations). It reads the
  topology on every event, so a configuration change takes effect at once; `Sim::topology_changed`
  drops state the change invalidated.
- **Frames are bytes.** Devices encode and decode real frames, so the event list exports straight
  to pcap and the decoders run on every hop. Serial links carry HDLC or PPP framing; the event list
  records the equivalent Ethernet II frame, because a capture file has one link type.
- **Events:** a binary heap of deliveries ordered by (time, sequence). Every delivery is recorded
  with the receiver's drop reason, if any (capped at `TRACE_CAP`), the event that caused it, and
  up to 8 decisions the receiver made (MAC table, routing lookup, TTL, ARP, access lists, NAT);
  `explain.rs` builds the OSI-layer view of PDU details from them.
- **Switches** learn (VLAN, MAC) → port, age entries, flood within the VLAN, tag on trunks except
  the native VLAN, treat an EtherChannel as one port (spreading frames over its members by
  address), enforce port security, and flush learned addresses when the topology changes. A
  frame that crosses `MAX_SWITCH_HOPS` switches is a loop: dropped and reported.
- **Routers and multilayer switches** forward by longest prefix, and between routes to the same
  prefix by administrative distance (connected 0, static 1 unless a distance is typed, EIGRP 90,
  OSPF 110, RIP 120), decrement TTL or hop limit, send ICMP/ICMPv6 errors, answer proxy ARP, apply
  ACLs, NAT, zone-pair and inspection policies, fragment to the outgoing IP MTU (or answer
  "fragmentation needed" when DF is set), and relay DHCP. On IPv4 a router drops the packet that starts ARP (hosts queue
  it); with IPv6 everyone holds the packet until neighbour discovery answers.
- **Computed protocols.** Spanning tree, EtherChannel negotiation, OSPF, RIP, EIGRP for
  IPv6, HSRP, VRRP, GLBP and VTP are computed from the topology the way the protocols converge,
  cached by a fingerprint of what they depend on, and recomputed when it changes. `control.rs`
  puts what they say on the wire at their usual intervals (BPDUs and RST BPDUs, OSPF hellos, RIP
  updates, VRRP advertisements, EIGRP for IPv6 hellos) for Simulation mode and captures.
- **Cellular.** `Topology::cell_refresh` (called after every change and on load) works out each phone's tower, sector, signal and address and keeps it in `Topology::cell_links`, which `Topology::peer` reads, so a phone's `Cellular0` port and its tower sector are the two ends of a link that no cable draws. `sim/src/cell.rs` shows the outcome as an attach request, accept or reject on the wire, bridges a tower's sectors to its backhaul (sector frames to the backhaul only; backhaul frames to the addressed sector), and the central office server then routes like any router.
- **Boards** run their BoardScript as simulated time advances: each slice executes at most a
  fixed number of steps, pauses at `sleep`, and reads or switches the things on its pins
  through the topology (IoT custom cables are not links; no frames cross them).
- **Ping/traceroute** send echo requests and run the event loop until the answer arrives or the
  timeout passes; DHCP, DNS, HTTP, Telnet/SSH, RESTCONF, NETCONF, IKEv2 and SIP do the same with
  their own exchanges. User PDUs (`pdu.rs`) are built field by field and sent once or
  periodically as `sim.advance` moves simulated time; IoT things report on their timers (and the shared room of `model::environment` is stepped a simulated second at a time) the
  same way.
- **Power.** IP phones and lightweight APs come up only when a PoE switch port within its
  budget (`poe.rs`) or their adapter powers them; a router or switch without a power supply in
  any bay is switched off; a cloud forwards a frame only to the port it
  is paired with, and modems are two-port bridges.

## Known duplication

None known. (Modular exponentiation used to exist twice, in `wire/src/dh.rs` for IKE and in
`wire/src/modp.rs` for TLS and SSH; `modp.rs` is now a thin layer over `dh.rs`.)

## The console

- `grammar.rs` is a table of command paths (keyword / argument / rest-of-line) with the modes and
  device kinds where each applies. One resolver handles abbreviations, ambiguity, `?` help and Tab
  completion; a test proves every entry is reachable by its full words.
- `exec.rs` dispatches resolved commands; a sub-mode falls back to global configuration commands
  (and leaves the sub-mode), as engineers expect.
- `running_config` renders re-playable lines; `reload` resets the device and replays its
  startup-config through the same dispatcher, so the configuration round-trips by construction.
  A switch's VLAN database and VTP settings survive a reload, as on real switches.

## The engine

- Every action is a command (`catalog.rs` lists them; `cmd.rs` runs them). Mutating commands take
  an undo snapshot of the topology.
- `Session::execute` wraps each command in `catch_unwind`; a panic restores the lab and resets the
  simulator. After each command it applies what the simulator decided (ports err-disabled by port
  security, sticky addresses) and synchronises VTP domains.
- Console lines run through `cli.exec`, which also keeps each device's scrollback.

## The UI

- `PacketApp::ui` draws menus, toolbars, the canvas, device windows, the Simulation Panel and the
  bottom device box into one `egui::Ui`.
- Host work (file dialogs, downloads, quitting) leaves as `UiRequest`s that the desktop and web
  shells serve.
- Icons and device pictures are drawn in code (`icons.rs`); colours come from `theme::Tokens`.
- Device-family pages live in their own modules: the instructor windows (`instructor.rs`), the
  touch home screen and front view of tablets and smartphones (`touch.rs`), and the board's
  Programming tab and Physical drawing (`board_page.rs`).
- `examples/snapshot.rs` renders the UI offscreen to a PNG, for checking UI work and for the
  README screenshots.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | Cellular network (M9.10): wire `cell`; model `cellular`; sim `cell`; ios `exec/cellular`; engine `cellular`; `Topology::cell_links` is derived state refreshed after every change. |
| 2026-11-13 | minor | BoardScript messages are English in `sim` and `engine` and translated at the edge (M9.11): the interface's `tr_any` matches catalog rows, the wrapper rows `line {}: {}` and `stopped: {}` are tried first and translate the message inside them. |
| 2026-11-13 | minor | The smart things' room (M9.11): `model/src/environment.rs` holds the settings and the exact integer physics, `sim/src/iot.rs` steps it from the devices that are on (`env_step`) before each report and gives the sensors their readings (`iot_reading`), `iot.environment` is the command. |
| 2026-11-13 | minor | `wire/src/dh.rs` gains MODP group 24 (explicit generator and subgroup order). |
| 2026-11-13 | minor | AH bundled with ESP (M9.5): `sim/src/vpn.rs` keeps the AH half of a bundle (`AhHalf`) next to the ESP one and unwraps AH before ESP on the way in; `sim/src/ikev2.rs` negotiates its child SA; `wire/src/ah.rs` gains `open_transport`. |
| 2026-11-13 | minor | IKE SA lifetimes and rekeying (M9.5): `sim/src/vpn.rs` keeps an ISAKMP or IKE SA's age and replacement next to the IPsec SA (a phase-1 rekey runs against its own exchange state and is merged when main mode completes); `sim/src/ikev2.rs` rekeys the IKE SA with CREATE_CHILD_SA; `wire/src/ikev2.rs` gains `Keys::rekey_with`. |
| 2026-11-13 | minor | The MCP lab-book test runs eleven labs, three of them troubleshooting labs. |
| 2026-11-13 | minor | Module added: wire `ec` (ECDH over P-256 and P-384, RFC 5903). |
| 2026-11-13 | minor | Module added: wire `ah` (the Authentication Header, RFC 4302). |
| 2026-11-13 | minor | Modules added: wire `des` (DES, 3DES, CBC) and `cipher` (AES, DES or 3DES behind one CBC interface). |
| 2026-11-13 | minor | One modular-exponentiation engine: `modp.rs` is a thin layer over `dh.rs`; the duplication note is removed. |
| 2026-10-31 | minor | Modules added: wire `dh`, `modp`, `suite`, `aes256`, `sha256`, `tls`, `ssh`, `peap`, `ripng`; model `board`, `psu`; sim `ikev1`, `rip6`, `peap`, `central`, `script`, `board`, `simmode`; ios `exec/swroute`; engine `instructor`, `board`; the lab-book runner and MCP test; ui `instructor`, `touch`, `board_page`. Boards and power supplies in the simulator; the duplicated modular exponentiation recorded. |
| 2026-10-30 | minor | Modules added: wire `http`, `netconf`, `xml`, `ipfrag`, `ikev2`, `vrrp`, `sip`; model `scenario`, `security`, `fhrp`, `automation`, `poe`, `cloud`; sim `pdu`, `explain`, `automation`, `zfw`, `mtu`, `ikev2`, `eigrp6`, `fhrp`, `sip`; ios `exec/` for EIGRP for IPv6, FHRP, IKEv2, router security, automation and device families; engine RESTCONF, NETCONF, controller, PDUs and power. Events record their decisions; forwarding applies zones, inspection and the IP MTU; power and clouds. |
| 2026-10-30 | minor | Layer table lists the modules added since (routing, security, services, wireless, voice, IoT, switch security, management); forwarding chooses by administrative distance. |
| 2026-10-30 | minor | Shared format; modules added since M0 (IPv6, serial framing, STP, EtherChannel, OSPF, RIP, VTP); simulator, console and engine notes brought up to date. |
| 2026-10-29 | major | Created. |
