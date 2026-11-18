# NetLabCraft lab book

> **Last reviewed:** 2026-11-18 · **Last updated:** 2026-11-18 · **Change:** minor (labs 50 to 52: OSPF adjacency states and the DR and BDR, flooding and LSA ageing, NSSA with nssa-only) · also merged with minor (lab 81: multiple spanning tree regions) · also merged with minor (lab 75: reverse Telnet through an octal card) · also merged with minor (labs 61 to 72: VPN, wireless, voice, IPv6, HSRP, DHCP snooping, ACL order and `debug` output) · also merged with minor (lab 49: OSPF authentication rollover, key chains and OSPFv3 with AH) · also merged with minor (labs 73 and 74: Python and JavaScript on boards) · also merged with minor (lab 48: Dial-up) · also merged with minor (lab 47: BGP session passwords and a conditional default route) · also merged with minor (lab 45: OSPF stub, NSSA and virtual-link areas) · also merged with minor (lab 46: route tags and BGP peer groups) · earlier: minor (lab 43: PPPoE with routers) · also merged with minor (lab 44: secure a wireless network) · earlier: minor (lab 41: PPPoE client and access concentrator) · earlier: minor (lab 42: BGP route reflectors) · earlier: minor (BGP communities, aggregation, timers and lab 40) · earlier: minor (lab 38: capture traffic with port mirroring) · earlier: minor (lab 39: redistribute between routing protocols and filter with a distribute list) · earlier: minor (lab 37: filter routes with a prefix list and a route map) · earlier: minor (lab 36: BGP between two autonomous systems) · earlier: minor (four more labs, 32 to 35: EIGRP, IPv6 routing, EtherChannel faults, port-security faults) · earlier: minor (an unnumbered cellular lab, M9.10) · earlier: minor (four more labs, 28 to 31: OSPF authentication, ICMP in ACLs, spanning-tree views, the OSPF database) · earlier: minor (seven troubleshooting labs, 21 to 27) · earlier: major (created: 20 labs over the three CCNA courses) · **Target:** Cisco Packet Tracer 9.0
Sixty-nine hands-on labs over the topics of the three CCNA courses, written for NetLabCraft
from scratch. Labs 1 to 20, 32, 33 and 36 to 49, 73 and 74 each build one topic; labs 21 to 29, 34, 35, 61 to 64 and 68 to 71 are troubleshooting labs: they
build a working network, break it with commands, and walk the symptoms, the `show` commands
that tell the faults apart, and the repairs; labs 30 and 31 are inspection labs that teach
reading a protocol's own views (spanning-tree roles and states, the OSPF database), the second
with a fault found from what is missing in the database; labs 65 to 67 and 72 read what `debug` prints (ICMP, ARP and routed packets, RIP and EIGRP, the spanning-tree root, NAT) and find the fault from a line that is there or one that is missing. Each lab starts from an empty lab and lists its
objectives, the topology with device models and an addressing table, the steps with every
command to type, and what to check with which command.

Every lab is also a script: its `lab`, `console` and `verify` blocks are run end to end, as
written, by `crates/engine/tests/lab_book.rs` (straight through the engine) and, for several
labs, by `apps/netlabcraft-cli/tests/mcp_lab_book.rs` (through `netlabcraft-cli mcp`, the way an
agent drives it; 19 of the 63). A lab that passes there works when typed by hand.

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

One more lab has no number because it belongs to no CCNA course: [Mobile data over a cell tower](cellular-mobile-data.md) (cell tower, central office server, APN and data plan, coverage; run by `lab_cellular_mobile_data`).

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
| 21 | [Troubleshooting: VLAN and trunk faults](21-troubleshoot-vlans-trunks.md) | CCNA 2 | wrong access VLAN, trunk allowed list, `show interfaces switchport` and `trunk`, MAC table |
| 22 | [Troubleshooting: static routes and host settings](22-troubleshoot-routing.md) | CCNA 2 | missing route, next hop off the link, wrong mask, proxy ARP, `traceroute` |
| 23 | [Troubleshooting: OSPF neighbours that will not form](23-troubleshoot-ospf.md) | CCNA 3 | passive interface, Hello interval, area mismatch, `show ip ospf neighbor` and `interface` |
| 24 | [Troubleshooting: an access list in the wrong place](24-troubleshoot-acls.md) | CCNA 3 | wrong direction, entry order, wrong port, match counters, `log` |
| 25 | [Troubleshooting: hosts that get no address, or the wrong one](25-troubleshoot-dhcp.md) | CCNA 2 | missing helper address, excluded range, wrong gateway, `show ip dhcp pool` |
| 26 | [Troubleshooting: NAT overload that does not translate](26-troubleshoot-nat.md) | CCNA 3 | inside and outside swapped, NAT ACL, missing `overload`, hits and misses |
| 27 | [Troubleshooting: the wrong switch is the spanning-tree root](27-troubleshoot-stp-root.md) | CCNA 2 | root election, `root primary`, blocked ports, root guard |
| 28 | [Troubleshooting: OSPF neighbours that disagree about authentication](28-troubleshoot-ospf-authentication.md) | CCNA 3 | MD5 key, key ID and authentication type mismatches, `show ip ospf interface`, `debug ip ospf adj` |
| 29 | [Troubleshooting: an ACL that breaks traceroute but not ping](29-troubleshoot-acl-icmp.md) | CCNA 3 | ICMP type and code in ACLs, match counters, inserting by sequence number, `traceroute` |
| 30 | [Inspecting spanning tree: roles, states and the blocked port per VLAN](30-inspect-spanning-tree.md) | CCNA 2 | `show spanning-tree brief`, `detail` and `interface`, per-VLAN roots, path cost |
| 31 | [Reading the OSPF database and the routing summaries](31-read-the-ospf-database.md) | CCNA 3 | router, network, summary and external LSAs, `show ip route summary`, `show ip protocols summary` |
| 32 | [EIGRP: neighbours, metrics, passive interfaces, variance and summaries](32-eigrp.md) | CCNA 3 | `network` and router ID, composite metric, feasible successors, `variance`, `passive-interface`, `ip summary-address eigrp` |
| 33 | [IPv6 routing: static routes, OSPFv3 and a default route](33-ipv6-routing.md) | CCNA 3 | `ipv6 route`, OSPFv3 with link-local next hops, `default-information originate`, `show ipv6 route`, IPv6 `tracert` |
| 34 | [Troubleshooting: an EtherChannel that will not bundle](34-troubleshoot-etherchannel.md) | CCNA 2 | passive/passive, `on` against LACP, a member with different settings, `show etherchannel summary` |
| 35 | [Troubleshooting: port security that shuts the wrong port](35-troubleshoot-port-security.md) | CCNA 2 | err-disabled ports, a hand-typed address, `restrict`, sticky, `show port-security`, `errdisable recovery` |
| 36 | [BGP between two autonomous systems](36-bgp.md) | CCNA 3 follow-up | `router bgp`, eBGP and iBGP sessions, `network` and the routing table, `next-hop-self`, `show ip bgp`, `clear ip bgp` |
| 37 | [Filter routes with a prefix list and a route map](37-bgp-prefix-lists-route-maps.md) | CCNA 3 follow-up | `ip prefix-list` with `ge` and `le`, `neighbor … prefix-list`, `route-map` with `match` and `set`, local preference, the two implicit denies, a name with no list |
| 38 | [Capture traffic with port mirroring](38-port-mirroring.md) | CCNA 2 follow-up | `monitor session` with `source interface`, `source vlan` and `destination interface`, `rx` and `tx`, ranges, the checks a session refuses, a Sniffer on the destination |
| 39 | [Redistribute between routing protocols and filter with a distribute list](39-redistribute-and-distribute-lists.md) | CCNA 3 follow-up | `redistribute` between RIP and EIGRP, `D EX` routes and distance 170, `default-metric`, a route map on a redistribution, `distribute-list` in and out, `show ip protocols` |
| 40 | [BGP communities, aggregation and timers](40-bgp-communities-aggregation.md) | CCNA 3 follow-up | `set community` and `send-community`, `ip bgp-community new-format`, community lists and `match community`, `no-export`, `aggregate-address` with `summary-only` and `as-set`, `neighbor … timers` |
| 41 | [PPPoE client and access concentrator](41-pppoe.md) | CCNA 3 follow-up | PADI, PADO, PADR, PADS and PADT, LCP, CHAP and PAP, IPCP, a session through a switch, IP in session frames |
| 42 | [BGP route reflectors](42-bgp-route-reflectors.md) | CCNA 3 follow-up | why iBGP needs a full mesh, `neighbor … route-reflector-client`, the client and non-client rules, ORIGINATOR_ID and CLUSTER_LIST, `bgp cluster-id`, the loop checks |
| 43 | [PPPoE with routers: an access concentrator and two clients](43-pppoe-with-routers.md) | CCNA 3 follow-up | `bba-group pppoe`, `interface virtual-template`, `peer default ip address pool`, `pppoe enable`, `interface dialer`, `ip address negotiated`, `pppoe-client dial-pool-number`, `show pppoe session` and `summary`, `clear pppoe all` |
| 44 | [Secure a wireless network](44-secure-a-wireless-network.md) | CCNA 1 and 2 follow-up | WEP, WPA2-PSK and WPA2 with a RADIUS server on an access point, matching the host's mode and encryption type, reading why a client does not associate, the coverage range |
| 45 | [OSPF stub, NSSA and virtual-link areas](45-ospf-stub-nssa-virtual-link.md) | CCNA 3 follow-up | `area N stub [no-summary]`, `area N default-cost`, the hello options that must match, `area N nssa [no-summary] [default-information-originate]`, type 7 and type 5 LSAs and the translator, `O N2` against `O E2`, `area N virtual-link`, `show ip ospf virtual-links`, LSA sequence numbers and ages |
| 46 | [Route tags and BGP peer groups](46-route-tags-and-bgp-peer-groups.md) | CCNA 3 follow-up | `ip route … tag`, `show ip route tag`, tags in OSPF external LSAs, `match tag` and `set tag`, `redistribute … tag`, `neighbor NAME peer-group`, members that keep their own settings, `show ip bgp peer-group` |
| 47 | [BGP session passwords and a conditional default route](47-bgp-passwords-and-conditional-default.md) | CCNA 3 follow-up | `neighbor … password` (TCP MD5), why a mismatch leaves a session in Active, what the console shows of a password, `service password-encryption`, `neighbor … default-originate`, `default-originate route-map`, a default route that follows a prefix list |
| 48 | [Dial-up: a modem, the telephone network and an access server](48-dial-up.md) | CCNA 3 follow-up | `cloud.number`, `modem inout` with a `bba-group` and a virtual template, `dialup.connect` and `dialup.disconnect`, a busy line, a missing number, a wrong password, a line that stops answering |
| 49 | [OSPF authentication: rolling a key over, key chains and OSPFv3 with AH](49-ospf-authentication-rollover-key-chains-ospfv3-ah.md) | CCNA 3 follow-up | `ip ospf message-digest-key` rollover, `key chain` with `send-lifetime` and `accept-lifetime`, `ip ospf authentication key-chain`, `show key chain`, `clock set`, `ospfv3 authentication ipsec spi`, `show ipv6 ospf neighbor` |
| 50 | [OSPF adjacency states and the DR and BDR](50-ospf-adjacency-states-and-dr-bdr.md) | CCNA 3 follow-up | `debug ip ospf adj`, `ip ospf priority`, `show ip ospf neighbor` (FULL/DR, FULL/BDR, 2WAY/OTHER), the five OSPF packet types in the event list |
| 51 | [OSPF flooding, retransmission and the life of an LSA](51-ospf-flooding-retransmission-and-lsa-ageing.md) | CCNA 3 follow-up | `show ip ospf neighbor detail` lists, `ip ospf retransmit-interval`, `debug ip ospf retransmission` and `flood`, sequence numbers over 1800 s |
| 52 | [OSPF NSSA: keeping a type 7 route inside the area](52-ospf-nssa-only-type-7-lsas.md) | CCNA 3 follow-up | the P bit, the translating border router, `redistribute … nssa-only`, `show ip ospf database nssa-external` |
| 57 | [BGP prefix limits and timers](57-bgp-prefix-limits-and-timers.md) | CCNA 3 follow-up | `maximum-prefix` ending a session, `warning-only`, `timers bgp`, `advertised-routes` and `routes` |
| 61 | [Troubleshooting: a site-to-site IPsec VPN that will not come up](61-troubleshoot-site-to-site-vpn.md) | CCNA 3 | a pre-shared key that differs, transform sets that differ, a crypto ACL that matches nothing, `show crypto isakmp sa` and `ipsec sa`, `clear crypto sa` |
| 62 | [Troubleshooting: a remote-access VPN client that cannot connect](62-troubleshoot-remote-access-vpn.md) | CCNA 3 | a server missing its transform set or pool, a wrong group, key, user or password, `vpnclient.connect` |
| 63 | [Troubleshooting: four laptops that will not join the Wi-Fi](63-troubleshoot-wireless-clients.md) | CCNA 2 | a mistyped SSID, WPA against WPA2, TKIP against AES, a laptop out of range, the reason a join is refused |
| 64 | [Troubleshooting: IP phones that will not register](64-troubleshoot-ip-phones.md) | CCNA 2 | voice VLAN, DHCP option 150, `telephony-service`, `max-ephones`, `ephone-dn`, `show ephone`, a call |
| 65 | [Reading `debug` output: ICMP, ARP and routed packets](65-debug-icmp-arp-and-packets.md) | CCNA 2 | `debug ip icmp`, `debug ip packet [ACL]`, `debug arp`, a missing return route, a wrong gateway, `undebug all` |
| 66 | [Reading `debug` output: RIP updates and EIGRP hellos](66-debug-rip-and-eigrp.md) | CCNA 3 | `debug ip rip [events]`, `debug ip routing`, `debug eigrp packets` and `neighbors`, a network not advertised, an AS mismatch |
| 67 | [Reading `debug` output: the spanning-tree root](67-debug-spanning-tree-root.md) | CCNA 2 | `debug spanning-tree bpdu`, `root` and `events`, a rogue root, root guard, `show spanning-tree inconsistentports` |
| 68 | [Troubleshooting: IPv6 that does not cross the routers](68-troubleshoot-ipv6.md) | CCNA 2 | `ipv6 unicast-routing`, a static route with a next hop nobody owns, a wrong host gateway, `show ipv6 route` |
| 69 | [Troubleshooting HSRP with `debug standby`](69-troubleshoot-hsrp.md) | CCNA 2 | a group number mismatch, hellos, a failover, `preempt`, `debug standby` |
| 70 | [Troubleshooting: DHCP snooping that blocks the real server](70-troubleshoot-dhcp-snooping.md) | CCNA 2 | an untrusted uplink, dropped DHCP messages, `show ip dhcp snooping` and `binding` |
| 71 | [Troubleshooting: an ACL entry that can never match](71-troubleshoot-acl-order.md) | CCNA 3 | match counters, entry order, `no SEQ`, `resequence`, `debug ip packet ACL` |
| 72 | [Reading `debug ip nat`: translations that never happen](72-debug-nat.md) | CCNA 3 | `debug ip nat [detailed]`, a wrong NAT ACL, a missing `ip nat outside`, ports and identifiers |
| 73 | [Python on a single-board computer](73-python-on-a-board.md) | IoT follow-up | the board's language, `analogRead`, `digitalWrite`, `time.sleep` in simulated time, `print`, run-time errors with their line, the step cap |
| 74 | [JavaScript on a microcontroller board](74-javascript-on-a-microcontroller.md) | IoT follow-up | a `loop` function the board calls again and again, `delay`, `console.log`, template literals, the subset's limits |
| 81 | [Multiple spanning tree: regions and one tree per group of VLANs](81-mst-regions.md) | CCNA 2 follow-up | `spanning-tree mode mst`, `spanning-tree mst configuration` with `name`, `revision` and `instance N vlan`, `spanning-tree mst N root primary`, `show spanning-tree mst`, a region boundary |
| 75 | [Reverse Telnet through an octal card](75-reverse-telnet-through-an-octal-card.md) | CCNA 3 follow-up | `Card-8A`, the octal cable, `line 1 8`, `transport input telnet`, `modem inout`, `telnet <address> 2001`, `show line`, `show users`, the escape sequence, `clear line`, the console password |

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
| 2026-11-18 | minor | Labs 50 to 52 join the book. OSPF adjacency states read with `debug ip ospf adj` and the DR and BDR of four routers on a switch, flooding with the request and retransmission lists, `ip ospf retransmit-interval` and an LSA's refresh, and `redistribute … nssa-only` in an NSSA. |
| 2026-11-18 | minor | Lab 81 (multiple spanning tree: regions and one tree per group of VLANs) joins the book; numbers 75 to 80 and 82 to 86 are reserved. |
| 2026-11-18 | minor | Lab 75 (reverse Telnet through an octal card) joins the book. |
| 2026-11-18 | minor | Labs 61 to 72 join the book: 63 labs with 49, 73 and 74. Site-to-site and remote-access VPN faults, wireless clients, IP phones, five labs on reading `debug` output, IPv6, HSRP, DHCP snooping and ACL order. |
| 2026-11-18 | minor | Lab 49 (OSPF authentication: rolling a key over, key chains and OSPFv3 with AH) joins the book: 49 labs. |
| 2026-11-16 | minor | Lab 48 (Dial-up) joins the book: 48 labs. |
| 2026-11-16 | minor | Lab 47 (BGP session passwords and a conditional default route) joins the book: 47 labs. |
| 2026-11-16 | minor | Lab 45 (OSPF stub, NSSA and virtual-link areas) joins the book: 46 labs. |
| 2026-11-16 | minor | Lab 46 (route tags and BGP peer groups) joins the book: 46 labs. |
| 2026-11-16 | minor | Lab 43 (PPPoE with routers) joins the book: 43 labs. |
| 2026-11-18 | minor | Labs 73 (Python on a single-board computer) and 74 (JavaScript on a microcontroller board), run end to end by the engine; numbers 49 to 72 are held by other work. |
| 2026-11-16 | minor | Lab 44 (secure a wireless network) joins the book: 43 numbered labs. |
| 2026-11-16 | minor | Lab 41: PPPoE client and access concentrator (discovery, LCP, CHAP and PAP, IPCP, IP in session frames, PADT), run end to end by the engine. |
| 2026-11-16 | minor | Lab 42 (BGP route reflectors) joins the book: 42 labs. |
| 2026-11-15 | minor | Four more labs (32 to 35): EIGRP (metrics, passive interfaces, feasible successors, variance, summaries), IPv6 routing (static routes, OSPFv3, a default route), an EtherChannel that will not bundle (three faults) and port security that shuts the wrong port; 32 and 34 also run over MCP. |
| 2026-11-13 | minor | An unnumbered lab, [Mobile data over a cell tower](cellular-mobile-data.md) (M9.10), run end to end by the engine (`lab_cellular_mobile_data`); it takes no number, because numbers 28 and up went to the troubleshooting and inspection labs. |
| 2026-11-13 | minor | Four more labs (28 to 31): OSPF authentication faults (a mistyped MD5 key, a different key ID, a different type), an ACL that drops ICMP time-exceeded so traceroute breaks and ping does not, spanning-tree inspection with the new `brief`, `detail` and `interface` views, and reading the OSPF database; 28 to 30 also run over MCP. The engine gaps they turned up are in [gaps.md](../gaps.md)' history. |
| 2026-11-13 | minor | Seven troubleshooting labs (21 to 27: 4 SRWE, 3 ENSA), run end to end by the engine, three of them also over MCP; the console views and options they turned up are in [gaps.md](../gaps.md)' history. |
| 2026-10-30 | major | Created: 20 labs (4 ITN, 9 SRWE, 7 ENSA), run end to end by the engine and over MCP. |
