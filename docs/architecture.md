# Architecture

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (modules for complex PDUs and the OSI view, automation, router security, IKEv2, FHRP, EIGRP for IPv6, SIP, PoE, clouds and scenarios)

How NetLabCraft is built. Rules for contributors and agents are in [`AGENTS.md`](../AGENTS.md);
the reasons behind the choices are in [decisions.md](decisions.md).

## Layers

`cargo xtask layers` enforces that each crate depends only on lower layers, and that nothing below
L6 depends on a UI toolkit.

| Layer | Crate | Role |
|---|---|---|
| L0 | `netlabcraft-core` | Value types: `MacAddr`, `Ipv4Net`, `Ipv6Net` (with EUI-64 and link-local helpers), `VlanId`, `VlanSet`, `DeviceId`. |
| L1 | `netlabcraft-wire` | Wire formats and checksums: Ethernet II/802.1Q, ARP, IPv4, ICMP, UDP, TCP, DHCP, DNS (`dns.rs`), IPv6/ICMPv6/NDP (`ipv6.rs`), HDLC and PPP framing (`serial.rs`), TCP (`tcp.rs`), DHCPv6, OSPF and OSPFv3 packets and LSAs, EIGRP, LACP, LLDP, RADIUS, TACACS+, SNMP, IKE and ESP (`ike.rs`, `vpn.rs`, with real AES-128-CBC and HMAC-SHA1-96 ESP), IKEv2 (`ikev2.rs`), VRRP v2/v3 (`vrrp.rs`), SIP with SDP (`sip.rs`), HTTP/1.1 with Basic login and percent-encoding (`http.rs`), NETCONF framing (`netconf.rs`), a small strict XML reader and writer (`xml.rs`, no DOCTYPE, size and depth caps), IPv4 fragmentation, reassembly and MSS clamping (`ipfrag.rs`), CAPWAP and 802.11 (`wlan.rs`, with the AES key wrap of group keys), the MD5, SHA-1 and AES code those need (`aes.rs` has the inverse cipher, CBC and key wrap), the pcap writer, and the one-line summaries the event list shows. Depends on `core` only. |
| L2 | `netlabcraft-model` | The lab document: devices, ports and their configuration, cables (a serial cable's first end is the DCE), the device catalogue (`models.rs`), access lists (`acl.rs`), server services (`services.rs`), interface names (`ifname.rs`), routing settings and loopbacks (`routing.rs`), switch security and switch QoS trust (`l2sec.rs`), management settings and flash (`mgmt.rs`), VPN with IKEv2 (`vpn.rs`), QoS, firewall, wireless, voice and SIP, IoT things and rules, cables (coaxial and phone too), modules and the physical view (`qos.rs`, `firewall.rs`, `wlan.rs`, `voice.rs`, `iot.rs`, `cable.rs`, `hardware.rs`, `physical.rs`), VRRP and GLBP (`fhrp.rs`), the zone-based firewall, inspection, IP MTU and MSS (`security.rs`), automation settings and the controller's routers (`automation.rs`), power over Ethernet (`poe.rs`), clouds (`cloud.rs`), scenarios and their PDUs (`scenario.rs`), activities (`activity.rs`), the `.nlcraft` format (`file.rs`). |
| L3 | `netlabcraft-sim` | The discrete-event simulator (`lib.rs`) and the protocols computed from the topology: spanning tree with the RSTP handshake (`stp.rs`), EtherChannel bundles (`channel.rs`), OSPF (`ospf.rs`, with its database in `ospf_lsdb.rs`), OSPFv3 (`ospf6.rs`), RIP (`rip.rs`), EIGRP (`eigrp.rs`) and EIGRP for IPv6 (`eigrp6.rs`), HSRP (`hsrp.rs`), VRRP and GLBP (`fhrp.rs`), VTP (`vtp.rs`); IPv6 forwarding, NDP and autoconfiguration (`v6.rs`); DHCP snooping with option 82, ARP inspection and edge protection (`l2sec.rs`); loopbacks (`loopback.rs`); the DHCP server (`dhcpsrv.rs`), logging levels and timestamps (`logcfg.rs`), NTP, syslog and SNMP (`mgmt.rs`, `snmp.rs`); the zone-based firewall and classic inspection (`zfw.rs`), IP MTU (`mtu.rs`), IKEv2 (`ikev2.rs`); SIP phones and call control (`sip.rs`); RESTCONF and NETCONF transports (`automation.rs`); user PDUs, simple and complex (`pdu.rs`); the decisions recorded per event and the OSI view built from them (`explain.rs`); email, FTP, VPN, QoS with shaping, firewall, wireless, voice and IoT with timed reports in their own modules. |
| L4 | `netlabcraft-ios` | Device consoles: the command grammar (`grammar.rs`), dispatch and modes (`exec.rs`), `show` output and running-config (`render.rs`), the PC/server shell (`host.rs`), output filters (`pipe.rs`); `exec/` holds the dispatch for routing, EIGRP for IPv6 (`eigrp6.rs`), VRRP and GLBP (`fhrp.rs`), switch security and QoS trust, management, VPN, IKEv2 (`ikev2.rs`), QoS, firewall, router security (`security.rs`: zones, inspection, MTU, MSS), automation (`automation.rs`), wireless, voice and the device families (`families.rs`: PoE, the switch card, SIP). |
| L5 | `netlabcraft-engine` | `Session` (lab + simulator + consoles + undo), the command catalog (`catalog.rs`, `cmd.rs`), sample labs (`sample.rs`); RESTCONF data and edits (`automation.rs`), NETCONF operations (`netconf.rs`) and the controller (`controller.rs`), all applying changes as console lines through the grammar; scenarios and PDUs (`pdu.rs`); power adapters and sources (`families.rs`). |
| L6 | `netlabcraft-ui-egui` | The egui interface. |
| L6 | `netlabcraft-mcp` | MCP server over stdio. |
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
- **Ping/traceroute** send echo requests and run the event loop until the answer arrives or the
  timeout passes; DHCP, DNS, HTTP, Telnet/SSH, RESTCONF, NETCONF, IKEv2 and SIP do the same with
  their own exchanges. User PDUs (`pdu.rs`) are built field by field and sent once or
  periodically as `sim.advance` moves simulated time; IoT things report on their timers the
  same way.
- **Power.** IP phones and lightweight APs come up only when a PoE switch port within its
  budget (`poe.rs`) or their adapter powers them; a cloud forwards a frame only to the port it
  is paired with, and modems are two-port bridges.

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
- `examples/snapshot.rs` renders the UI offscreen to a PNG, for checking UI work and for the
  README screenshots.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Modules added: wire `http`, `netconf`, `xml`, `ipfrag`, `ikev2`, `vrrp`, `sip`; model `scenario`, `security`, `fhrp`, `automation`, `poe`, `cloud`; sim `pdu`, `explain`, `automation`, `zfw`, `mtu`, `ikev2`, `eigrp6`, `fhrp`, `sip`; ios `exec/` for EIGRP for IPv6, FHRP, IKEv2, router security, automation and device families; engine RESTCONF, NETCONF, controller, PDUs and power. Events record their decisions; forwarding applies zones, inspection and the IP MTU; power and clouds. |
| 2026-10-30 | minor | Layer table lists the modules added since (routing, security, services, wireless, voice, IoT, switch security, management); forwarding chooses by administrative distance. |
| 2026-10-30 | minor | Shared format; modules added since M0 (IPv6, serial framing, STP, EtherChannel, OSPF, RIP, VTP); simulator, console and engine notes brought up to date. |
| 2026-10-29 | major | Created. |
