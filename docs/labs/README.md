# NetLabCraft lab book

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (created: 20 labs over the three CCNA courses) · **Target:** Cisco Packet Tracer 9.0

Twenty hands-on labs over the topics of the three CCNA courses, written for NetLabCraft from
scratch. Each lab starts from an empty lab and lists its objectives, the topology with device
models and an addressing table, the steps with every command to type, and what to check with
which command.

Every lab is also a script: its `lab`, `console` and `verify` blocks are run end to end, as
written, by `crates/engine/tests/lab_book.rs` (straight through the engine) and, for several
labs, by `apps/netlabcraft-cli/tests/mcp_lab_book.rs` (through `netlabcraft-cli mcp`, the way an
agent drives it). A lab that passes there works when typed by hand.

## How to read a lab

- **`lab` blocks** are engine commands (`device.add`, `link.add`, `sim.advance`…), the same as
  the Insert menu, the Simulation panel or a dialog. Type them into `netlabcraft-cli run --cmd`,
  send them over MCP with `netlabcraft_run`, or do the same thing with the mouse.
- **`console DEVICE` blocks** are typed at that device: the CLI tab of a router or switch, the
  Command Prompt of a PC or server. Over MCP, `netlabcraft_cli` with the device and the lines.
- **`verify` blocks** say what to check. `R1# show ip route` followed by `expect: text` means
  the output should contain that text (`reject:` means it shouldn't). `ping PC1 10.0.0.1 =>
  success` means the ping should be answered (`failure`: it shouldn't). `events: ARP` means the
  Simulation event list should hold such an event. `run snmp.request {…}` runs an engine command
  and checks its answer.

Times in Realtime mode pass by themselves; `sim.advance` stands for waiting (spanning tree's
30 seconds, OSPF's dead interval, NTP's first poll).

## The labs

| # | Lab | Course | Topics |
|---|---|---|---|
| 1 | [Secure a new switch and router](01-device-basics.md) | CCNA 1 | hostname, passwords, banner, SVI, SSH with local users |
| 2 | [IPv4 and IPv6 on two LANs](02-dual-stack-addressing.md) | CCNA 1 | dual stack, link-local, SLAAC, verification |
| 3 | [Subnet a small company with VLSM](03-vlsm-subnetting.md) | CCNA 1 | VLSM plan, static and default routes |
| 4 | [Watch ARP, ICMP and DHCP in Simulation mode](04-arp-icmp-dhcp-simulation.md) | CCNA 1 | DHCP DORA, ARP, ICMP, the event list |
| 5 | [VLANs and 802.1Q trunks](05-vlans-and-trunks.md) | CCNA 2 | VLANs, access ports, trunk, native VLAN, DTP off |
| 6 | [Inter-VLAN routing](06-inter-vlan-routing.md) | CCNA 2 | router on a stick, multilayer switch, routed port |
| 7 | [Spanning tree tuning](07-stp-tuning.md) | CCNA 2 | root bridge, port cost, Rapid PVST+, PortFast, BPDU guard |
| 8 | [EtherChannel with LACP](08-etherchannel-lacp.md) | CCNA 2 | LACP active/passive, port-channel trunk |
| 9 | [DHCPv4 and DHCPv6](09-dhcpv4-dhcpv6.md) | CCNA 2 | DHCP server and relay, SLAAC, stateless and stateful DHCPv6 |
| 10 | [First-hop redundancy with HSRP](10-hsrp.md) | CCNA 2 | HSRPv2, priority, preempt, interface tracking |
| 11 | [Switch security](11-switch-security.md) | CCNA 2 | port security, DHCP snooping, Dynamic ARP Inspection |
| 12 | [A wireless LAN with a controller](12-wlan-controller.md) | CCNA 2 | WLC, lightweight AP, WPA2-PSK WLANs per VLAN |
| 13 | [Static and floating static routes](13-static-floating-routes.md) | CCNA 2 | IPv4 and IPv6 static, default and floating routes |
| 14 | [OSPFv2](14-ospf.md) | CCNA 3 | network and interface commands, DR election, timers, multi-area |
| 15 | [Standard and extended ACLs](15-acls.md) | CCNA 3 | numbered and named ACLs, sequence edits, VTY access-class |
| 16 | [NAT and PAT](16-nat-pat.md) | CCNA 3 | static NAT, dynamic pool, PAT overload |
| 17 | [GRE over IPsec](17-gre-over-ipsec.md) | CCNA 3 | GRE tunnel, OSPF through it, ISAKMP, transform set, crypto map |
| 18 | [QoS marking and policing](18-qos.md) | CCNA 3 | class maps, DSCP marking, policing |
| 19 | [Network management](19-network-management.md) | CCNA 3 | CDP, LLDP, NTP, syslog, SNMP, TFTP backups |
| 20 | [RESTCONF from the Programming tab](20-restconf.md) | CCNA 3 | RESTCONF GET, PATCH, POST, DELETE |

## Running the scripts

```sh
CARGO_TARGET_DIR=… cargo test -p netlabcraft-engine --test lab_book          # all labs, engine
CARGO_TARGET_DIR=… cargo test -p netlabcraft-cli --test mcp_lab_book         # several, over MCP
LAB_BOOK_TRACE=1 cargo test -p netlabcraft-engine --test lab_book lab_14 -- --nocapture   # read a run
```

A failing step names its line in the lab's Markdown.

## Known limitations

- Outputs are NetLabCraft's own wording, not a copy of any vendor's: the labs' `expect:` lines
  quote NetLabCraft's output. The facts they show (addresses, roles, states, counters) are the
  ones a student is asked to find.
- `show standby brief` prints the same per-group report as `show standby`, not a one-line
  table.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | major | Created: 20 labs (4 ITN, 9 SRWE, 7 ENSA), run end to end by the engine and over MCP. |
