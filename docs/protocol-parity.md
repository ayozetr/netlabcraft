# Protocol and command parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (the CCNA 2 and 3 depth round; section figures aligned with ROADMAP.md) · **Target:** Cisco Packet Tracer 9.0 (as used in CCNA courses)

What the simulated devices do, protocol by protocol, against the reference app. Part of
[target-app-parity.md](target-app-parity.md). Behaviour comes from the standards named in each
row and from black-box observation; console wording and table layouts are our own. Every
console command is listed in [`command-reference.md`](command-reference.md) (generated).

**Status:** ✅ works · 🟡 partly · ❌ not yet.

## Layer 2

**Layer 2: ~90% ready, 6–10 h** (estimated; the same figures as the Layer 2 row of [`ROADMAP.md`](../ROADMAP.md#dimensions)).

| Feature | Standard | Status | Notes and gap |
|---|---|---|---|
| Ethernet II and 802.1Q frames | IEEE 802.3, 802.1Q | ✅ | Real encoded frames, padded to 64 bytes. |
| MAC learning and aging | IEEE 802.1D | ✅ | Per (VLAN, MAC); flushed when the topology changes. `show mac address-table`, `clear mac address-table`. |
| VLANs, access and trunk ports | IEEE 802.1Q | ✅ | Native VLAN, allowed lists, `show vlan brief`, `show vlan id N`, `show vlan summary`, `show interfaces trunk`; `switchport trunk encapsulation dot1q` kept on multilayer switches and refused with an explanation on access switches (ISL refused). |
| Spanning tree (per VLAN) | IEEE 802.1D | 🟡 | Roles, priorities, `root primary/secondary`, portfast, amber lights; computed at once; configuration BPDUs (IEEE format, tagged per VLAN on trunks) every 2 s from designated ports; a port that starts forwarding listens 15 s and learns 15 s (LIS/LRN in `show spanning-tree`, amber light) unless portfast or `spanning-tree mode rapid-pvst`. `spanning-tree [vlan N] cost` and `port-priority` change the election (the priority travels in the BPDU's port id); `spanning-tree portfast default`; BPDU guard per port or `portfast bpduguard default` (a BPDU or a cabled switch err-disables the port with a log line); root guard (a better root makes the port root-inconsistent); `link-type` kept and shown; `show spanning-tree summary`, `show spanning-tree vlan N detail`. No loop guard, max-age wait or RSTP proposal/agreement frames: 4–6 h. |
| EtherChannel | IEEE 802.1AX (LACP); PAgP | ✅ | `on`, `active/passive`, `desirable/auto`; suspended members; one STP port per bundle; traffic spread by address; `interface port-channel`, `port-channel load-balance src\|dst\|src-dst mac\|ip` (picks the member a flow takes), `lacp port-priority`, `show etherchannel summary\|load-balance\|port-channel`, `show lacp neighbor`; LACPDUs every 30 s (actor and partner, sync state) in Simulation mode and captures. PAgP has no frames: its format isn't public. |
| VTP | — | 🟡 | Server/client/transparent, domain, password, versions 1/2, revisions. No pruning, version 3 or advertisement frames: 4–6 h. |
| DTP | — | ✅ | `switchport mode dynamic auto/desirable`, `switchport nonegotiate`; the working mode is negotiated from both ends and shown in `show interfaces X switchport`, `show interfaces trunk`, `show vlan brief`, `show dtp`. Ports default to access (real access switches default to dynamic auto). No DTP frames. |
| Port security | — | ✅ | Maximum, violation shutdown/restrict/protect, sticky and static addresses, `aging time` and `aging type absolute\|inactivity`, `show port-security`. |
| Errdisable recovery | — | ✅ | `errdisable recovery cause bpduguard\|psecure-violation\|dhcp-rate-limit\|arp-inspection\|all` and `interval N` bring ports back on simulated time; `show errdisable recovery`, `show interfaces status err-disabled`. |
| DHCP snooping | — | 🟡 | `ip dhcp snooping [vlan …]`, `ip dhcp snooping trust`, `limit rate` (over the limit err-disables the port); untrusted ports drop server answers (OFFER, ACK, NAK) with a drop reason; ACKs through trusted ports build the binding table; `show ip dhcp snooping [binding]`, `clear ip dhcp snooping binding`. `ip dhcp snooping information option` is kept but option 82 is not inserted: 2–3 h. |
| Dynamic ARP inspection | — | ✅ | `ip arp inspection vlan`, `trust`, `validate src-mac\|dst-mac\|ip`, `limit rate`; ARP from untrusted ports checked against the snooping bindings, with a drop reason; `show ip arp inspection`. |
| Hubs | IEEE 802.3 | ✅ | Repeat every frame. |
| Switch management (SVI, default gateway) | — | ✅ | `interface vlan N`, `ip default-gateway`, `show interfaces counters` (bytes and unicast, multicast and broadcast frames per port), `sdm prefer` (takes effect after a reload, `show sdm prefer`). |

## Layer 3

**Layer 3: ~92% ready, 10–16 h** (estimated; the Layer 3 row of [`ROADMAP.md`](../ROADMAP.md#dimensions)).

| Feature | Standard | Status | Notes and gap |
|---|---|---|---|
| IPv4, ARP, ICMP | RFC 791, 826, 792 | ✅ | TTL, time exceeded, unreachable (net, host, port, admin), proxy ARP. Routers drop the packet that starts ARP, as real ones do. |
| Static routes | — | ✅ | Next hop or exit interface, default route, longest prefix match; an administrative distance after the route makes it a floating static that waits behind a dynamic route and takes over when that disappears (floating defaults too; typing the route again changes its distance); `show ip route` shows `[distance/metric]`. |
| Loopback interfaces | — | ✅ | `interface loopback N` (created on first use, `no interface loopback N`) with IPv4 and IPv6 addresses, `shutdown`, `description`; always up unless shut; pings to it answer; OSPF advertises it as a /32 (or its subnet with `ip ospf network point-to-point`), RIP and EIGRP when a network statement covers it; the highest loopback address is the default OSPF, OSPFv3 and EIGRP router ID. |
| Router-on-a-stick | IEEE 802.1Q | ✅ | Subinterfaces with `encapsulation dot1Q`. |
| Multilayer switching | — | ✅ | `ip routing`, VLAN interfaces, routed ports (`no switchport`), static routes, RIP, OSPF, EIGRP; `ip helper-address` and access lists on VLAN interfaces. |
| RIP | RFC 1058, RFC 2453 | 🟡 | Versions 1/2 (both classless here), classful `network`, passive interfaces, split horizon, 15 hops, default origination. RIP responses every 30 s (multicast for v2, broadcast for v1) with split horizon; also on multilayer switches. No triggered updates or auto-summary: 4–6 h. |
| OSPFv2 | RFC 2328 | 🟡 | Neighbours, SPF, router id, `network` statements or `ip ospf N area A` on interfaces (which also starts the process), passive interfaces and `passive-interface default` with `no passive-interface` exceptions, default origination, `redistribute static` (E2 routes), cost 1 on Ethernet and 64 on serial, `maximum-paths` with equal-cost paths installed. `ip ospf hello-interval` and `dead-interval` travel in the hellos, and a mismatch blocks the adjacency and is named in `show ip ospf neighbor` and `debug ip ospf`; `ip ospf network point-to-point` (no DR/BDR on the link); the DR election is not preemptive, and `clear ip ospf process` elects again; `show ip ospf database` (router, network, summary and external LSAs) and `show ip ospf neighbor detail`. Hellos every 10 s; `ip ospf cost`, `ip ospf priority` with DR/BDR election (priority 0 never DR, none on serial links), `auto-cost reference-bandwidth`, several areas with inter-area routes shown as `O IA`, `show ip ospf`, `show ip ospf interface [brief]`, DR/BDR in `show ip ospf neighbor`; also on multilayer switches. When neighbours meet, each sends its database description, a link-state request, a link-state update with its router-LSA (with a valid Fletcher checksum) and an acknowledgement, visible in Simulation mode and captures. `area N stub` (both ends must agree, or no neighbours; the border router sends `O*IA` default routes in place of external ones), `area N stub no-summary`, `area N range` on border routers. No NSSA (beyond CCNA), virtual links, or network/summary LSAs on the wire: 6–10 h. |
| EIGRP | RFC 7868 | 🟡 | `router eigrp N` (one AS per router), `network` with or without wildcard, `passive-interface`, `eigrp router-id`, `bandwidth` and `delay`; composite metric; DUAL successors and feasible successors; `maximum-paths` and `variance` with per-flow load balancing (every path in `show ip route`); hellos and hold time per interface (`ip hello-interval\|hold-time eigrp`), silent neighbours dropped after the hold time, goodbyes; updates, queries, replies and acks on the wire; `show ip eigrp neighbors\|topology\|interfaces`, distance 90; `debug eigrp packets`. No stuck-in-active timer, auto-summary or EIGRP for IPv6: 4–6 h. |
| IPv6 addressing | RFC 8200, RFC 4291 | ✅ | Global, EUI-64 and link-local addresses, `ipv6 enable`. |
| NDP and SLAAC | RFC 4861, RFC 4862 | ✅ | Neighbour solicitation/advertisement, router solicitation/advertisement with the M/O flags (see DHCPv6), host autoconfiguration; routers take SLAAC addresses too (`ipv6 address autoconfig [default]`, the default adding an ND ::/0 route at distance 2). No duplicate address detection. |
| ICMPv6 | RFC 4443 | ✅ | Echo, time exceeded, unreachable; ping and traceroute. |
| IPv6 static routes | — | ✅ | Next hop, interface, or link-local next hop with its interface. |
| DHCPv6 | RFC 8415 | ✅ | `ipv6 dhcp pool` (address prefix, DNS, domain), `ipv6 dhcp server`, `ipv6 nd managed-config-flag` / `other-config-flag`; hosts follow the router: SLAAC, SLAAC with stateless DHCPv6, or stateful DHCPv6 (Solicit, Advertise, Request, Reply on the wire); `show ipv6 dhcp pool/binding`; `ipv6 dhcp relay destination` (Relay-forward and Relay-reply on the wire; the server picks the pool from the relay's link address); routers as stateful clients (`ipv6 address dhcp`, asked for again after `no shutdown` and reloads). Router clients don't get a /128 as real ones do: 1–2 h. |
| OSPFv3 | RFC 5340 | ✅ | `ipv6 router ospf N` (router ID, passive interfaces, default origination), `ipv6 ospf N area A` on ports, subinterfaces and VLAN interfaces; neighbours over link-local addresses, several areas (`OI`), external default (`OE2`); hellos to ff02::5 every 10 s on the wire; `show ipv6 ospf [neighbor]`. No DR election shown, LSAs on the wire or area types. |
| IPv6 on subinterfaces and VLAN interfaces | — | ✅ | Router-on-a-stick subinterfaces (tagged), VLAN interfaces and routed ports of multilayer switches (`ipv6 unicast-routing` with `ip routing`), with addresses, SLAAC, DHCPv6 server and traffic filters; `show ipv6 interface brief` lists them. |
| FHRP (HSRP) | RFC 2281 | ✅ | `standby N ip/priority/preempt` on router interfaces, subinterfaces and multilayer VLAN interfaces; active answers ARP with the virtual MAC; hellos every 3 s; `standby version 2` (virtual MAC 0000.0c9f.fXXX, hellos to 224.0.0.102); `standby N track INTERFACE [decrement D]` (the priority drops while the interface is down, so the other router takes over, and `show standby` says why); `show standby [brief]`. Election computed (no coup/resign messages). VRRP and GLBP are concept only: 6–10 h. |

## Services

**Services: ~94% ready, 4–8 h** (estimated; the Services row of [`ROADMAP.md`](../ROADMAP.md#dimensions)).

| Feature | Standard | Status | Notes and gap |
|---|---|---|---|
| DHCPv4 server on routers | RFC 2131 | ✅ | Pools, exclusions, bindings, conflicts; `lease DAYS [HOURS [MIN]] \| infinite` and `domain-name` (options 51 and 15, shown by PCs in `ipconfig /all`); `show ip dhcp binding/pool/conflict/server statistics`, `clear ip dhcp binding`, `clear ip dhcp conflict *`, `clear ip dhcp server statistics`, `debug ip dhcp server packet`; clients send a second DISCOVER when the first goes unanswered. |
| DHCPv4 relay | RFC 2131 | ✅ | `ip helper-address` on router interfaces, subinterfaces and multilayer VLAN interfaces. |
| DHCPv4 on servers | RFC 2131 | ✅ | Services tab pools. |
| DHCP client | RFC 2131 | ✅ | Hosts: `ipconfig /renew`, IP Configuration DHCP option. Routers and routed ports: `ip address dhcp` (leased at `no shutdown` or at once when up; the lease's router becomes a default route that any static default overrides); the home router's Internet port. |
| DNS | RFC 1035 | ✅ | A records on servers; `nslookup`, names in ping/tracert/browser. |
| HTTP | RFC 9110 over TCP (RFC 9293) | ✅ | Server file manager, PC web browser. No HTTPS: 6–10 h. |
| Telnet and SSH | RFC 854, RFC 4253 | ✅ | Local users, `login local`, `transport input`, RSA keys, `ip ssh version`. The SSH session is simulated, not encrypted. |
| TFTP | RFC 1350 | ✅ | Server TFTP service with a file list; `copy running-config\|startup-config tftp: <address> [file]`, `copy tftp: running-config\|startup-config <address> <file>`; IOS image backups and restores with `copy flash: tftp:` and `copy tftp: flash:`, `boot system flash:NAME`; 512-byte blocks and acknowledgements on the wire (the server answers from port 69). |
| FTP | RFC 959 | ✅ | Server FTP service with accounts (read, write, delete, list) and files; the PC `ftp` client (`put`, `get`, `dir`, `delete`, `quit`) and `dir` for the PC's own files; `copy running-config\|startup-config ftp:` and back with `ip ftp username/password`. Control and passive data connections over TCP on the wire (each command is its own session); the password is never shown in the event list. No active mode or `rename`. |
| Email (SMTP/POP3) | RFC 5321, RFC 1939 | ✅ | The server's EMAIL service (domain, users, mailboxes saved with the lab); the PC Desktop Email app (account, inbox, compose, receive); SMTP and POP3 sessions over TCP on the wire (the password masked); fetched mail leaves the server. A server relays mail for its own users to another domain's server, found through its DNS server's A record for the domain (no MX records); failed deliveries come back as a bounce; it refuses to relay for anyone else. |
| NTP | RFC 5905 | ✅ | Server NTP service; `ntp server` syncs at once and every 64 s; `ntp master [stratum]` on routers and switches; `ntp authenticate`, `ntp authentication-key N md5 KEY`, `ntp trusted-key N`, `ntp server A key N` with MD5 signatures (a wrong or untrusted key keeps the client unsynchronised); `ntp update-calendar`; `show ntp status/associations`. |
| Syslog | RFC 5424 | ✅ | `logging host` sends log messages to the server, whose SYSLOG page lists them; `logging trap\|console\|buffered <level>` filter by severity; `service timestamps log\|debug datetime [msec]` stamps them with the device clock. |
| AAA with RADIUS | RFC 2865 | ✅ | `aaa new-model`, `aaa authentication login default group radius local`, `radius-server host X key K`, `login authentication default`; vty logins (Telnet, SSH) ask the server; Access-Request with the password hidden and signed answers on the wire (MD5, RFC 1321), retransmitted; a wrong secret or no answer falls back to the next method; the Server's AAA service holds clients and users. Console logins keep their line settings. |
| TACACS+ | RFC 8907 | ✅ | `tacacs-server host X key K`, `group tacacs+` in the login method list; the Server's AAA service answers on TCP 49 with the same clients and users; a PAP login per exchange with the body encrypted by the key's MD5 pad; an answer that doesn't read with the key falls back to the next method. |
| SNMP | RFC 3416 | ✅ | SNMPv2c agents on routers and switches (`snmp-server community NAME ro\|rw`, `location`, `contact`, `show snmp`): get, get-next and set on the system group and the interfaces table; a set of sysName, sysContact or sysLocation with a read-write community changes the device; a wrong community gets no answer. The PC's MIB Browser (get, get-next, set, walk, v2c or v3). v2c traps (`snmp-server host`, `snmp-server enable traps` linkdown, linkup, coldstart) received and listed by PCs and servers (MIB Browser ▸ Traps, `snmp.traps`). SNMPv3 with users and groups, noAuthNoPriv and authNoPriv (HMAC-MD5/SHA-96, engine discovery, Report PDUs). No v3 privacy, v3 or v1 traps, informs: 3–5 h. |

## Security

**Security: ~86% ready, 15–25 h** (estimated; the Security row of [`ROADMAP.md`](../ROADMAP.md#dimensions)).

| Feature | Status | Notes and gap |
|---|---|---|
| Standard and extended ACLs, numbered and named | ✅ | `ip access-group in/out` (also on multilayer VLAN interfaces), match counters, implicit deny, TCP/UDP ports; sequence numbers (`10 permit …`, `no 10`, shown in `show access-lists`), `ip access-list resequence NAME START STEP`, `established` on TCP lines, ICMP types (echo, echo-reply, unreachable, time-exceeded), remarks kept with the list. |
| `access-class` on vty lines | ✅ | Telnet/SSH logins refused (TCP reset) when the list denies the source; counted in `show access-lists`. |
| IPv6 ACLs | ✅ | `ipv6 access-list` with `permit\|deny ipv6\|icmp\|tcp\|udp` and any/host/prefix addresses and ports, `ipv6 traffic-filter in\|out`, neighbour discovery passes implicitly, `show ipv6 access-list` with matches; denied packets get an administratively-prohibited unreachable. |
| NAT: static, pool, PAT (overload) | ✅ | ICMP, UDP and TCP; `show ip nat translations`, `show ip nat statistics`. ICMP errors about translated packets are translated back (traceroute works through PAT). |
| Passwords and lines | ✅ | `enable secret/password`, console and vty passwords with real prompts, `banner motd`, `service password-encryption`, `security passwords min-length`, `login block-for … attempts … within …` (`show login`), `username … privilege 15` (logs in to privileged mode), `ip ssh time-out` and `authentication-retries`; password recovery with `config-register 0x2142` (the startup configuration is skipped at reload) and `copy startup-config running-config`. |
| PPP authentication (PAP/CHAP) | ✅ | `ppp authentication chap\|pap` (one or both, in order), `ppp pap sent-username`, checked against `username` entries (CHAP: both routers name each other with the same secret); failure keeps the line protocol down and `show interfaces` says why. No LCP/CHAP frames on the wire. |
| GRE tunnels | 🟡 | `interface tunnel N` with `tunnel source\|destination`, `tunnel mode gre ip`; RFC 2784 headers; the line protocol follows the source and a route to the destination, connected, static or learned by OSPF, RIP or EIGRP; a destination learned through the tunnel itself takes it down (recursive routing); static routes and routing protocols over the tunnel. No keepalives, `ip mtu`, ACLs or NAT on tunnels, or IPv6: 3–5 h. |
| IPsec site-to-site | 🟡 | `crypto isakmp policy` and `key`, transform sets, crypto maps applied to interfaces; the first interesting packet starts IKEv1 main mode (6 messages) and quick mode (3) on UDP 500, and a mismatch stops at the message where it shows (policies, keys, transform sets) with a notify and the reason in `show crypto isakmp sa`; ESP (RFC 4303 layout) once quick mode completes. Ciphers are stand-ins, not real encryption; no transport mode or IKEv2: 6–10 h. |
| QoS (MQC) | 🟡 | `class-map`, `policy-map` with `set dscp\|precedence`, `police`, `priority`, `bandwidth`; `service-policy input\|output`; DSCP marking in the IPv4 header; policing on simulated time; output queues on congested links (priority first within its rate, the rest by weight, tail drop with a reason, queuing delay); `show policy-map interface` with counters, queue depth and drops. `shape average` is accepted and shown but delays nothing; no `mls qos trust`: 4–6 h. |
| Firewall appliance | 🟡 | The Firewall 5GE model: security levels, a stateful connection table, access rules with object groups, object and twice NAT, a DHCP server, `inspect icmp` ([hardware-parity.md](hardware-parity.md)). More inspections: 4–6 h. |
| Zone-based firewall and `ip inspect` on routers | ❌ | 8–12 h. |

## WAN

| Feature | Status | Notes and gap |
|---|---|---|
| Serial links (DCE/DTE, clock rate) | ✅ | The first end cabled is the DCE; no clock or mismatched framing keeps the line protocol down; `show controllers`. |
| HDLC and PPP framing | ✅ | On the wire; listed and captured as the equivalent Ethernet II frame (a capture has one link type). |
| Frame Relay, DSL/cable modems, cloud | ❌ | Low priority in current courses. |

## Console

**Device console: ~82% ready, 25–40 h** (estimated; the Device console row of [`ROADMAP.md`](../ROADMAP.md#dimensions)).

| Feature | Status | Notes and gap |
|---|---|---|
| Modes: user, privileged, global, interface, subinterface, range, loopback, tunnel, VLAN, line, DHCP and DHCPv6 pools, router OSPF/RIP/EIGRP, OSPFv3, IPv4 and IPv6 ACLs, VLAN interface, ISAKMP policy, crypto map, transform set, class map, policy map, telephony, firewall objects (26 in all) | ✅ | Prompts follow the usual `name>`, `name#`, `name(config)#` convention. |
| Abbreviations, `?` help, Tab completion, history, `do` | ✅ | Ambiguity and unknown-word errors point at the word with a `^` marker that counts the prompt; `?` after a first word summarises it; `history size N` on lines and `terminal history size N`; extended `ping A [repeat N] [size S] [source IF\|A.B.C.D] [timeout T]`. |
| Running/startup config, `write`, `copy`, `erase`, `reload` | ✅ | Configuration round-trips through the same dispatcher (tested); `show file systems`, `dir`, `boot system`, `config-register`. |
| `show` coverage | 🟡 | Interfaces (also `status`, `X switchport`), IP/IPv6 interface and routes, ARP, MAC table, VLANs, trunks, STP, EtherChannel, VTP, port security, OSPF (also `interface`), RIP, DHCP, NAT, ACLs, SSH, controllers (Ethernet and serial), CDP, LLDP, HSRP (`standby`), logging, version, flash, clock, users, history, processes, memory, protocols, `interfaces description`, `interfaces vlan N`, `ipv6 interface X`, `ip route A.B.C.D` and by source, login, privilege, ssh, sessions, hosts, boot, environment, inventory, tech-support, `running-config interface X`, `ip traffic`, `tcp brief`, `interfaces counters`, `vlan id\|summary`, `dtp`, `spanning-tree summary\|detail`, `etherchannel load-balance\|port-channel`, `lacp neighbor`, `ip ospf database`, `ip ospf neighbor detail`, `ip dhcp conflict\|server statistics`, `ip dhcp snooping`, `ip arp inspection`, `ip nat statistics`, `errdisable recovery`, `sdm prefer`, `file systems`; output filters `\| include`, `exclude`, `begin`, `section`, `count`. What is still missing turns up as the labs are walked ([gaps.md](gaps.md), item 1). |
| CDP | 🟡 | `cdp run`, `cdp enable`, `show cdp`, `show cdp neighbors [detail]`; computed, no advertisement frames (the CDP frame format is proprietary). |
| LLDP (IEEE 802.1AB) | ✅ | `lldp run`, `lldp transmit`/`receive` per port, `show lldp`, `show lldp neighbors [detail]`, `show lldp entry *`; real LLDP frames every 30 s in Simulation mode and captures. |
| `clock set`, `terminal`, `debug` | 🟡 | `clock set` and `show clock` (per device, advances with simulated time, lost on reload); `terminal length/width/monitor` accepted (output is never paged); `debug ip icmp`, `debug ip rip`, `debug ip ospf adj\|events\|hello` (the hellos heard), `debug ip packet` (each routed or dropped packet), `debug ip nat` (new translations), `debug ip dhcp server events` (leases given and released), `debug ip dhcp server packet`, `undebug all`, `show debugging`; `terminal history size`. |
| Logging and console messages | ✅ | A logging buffer per device (`show logging`, `clear logging`): link up/down, port-security and BPDU-guard shutdowns, configuration changes, debug output; the same messages appear on the device's console after the prompt, and go to syslog servers (`logging host`); each destination filtered by severity, with optional timestamps. |
| Host shell (PC Command Prompt) | ✅ | `ipconfig [/all\|/renew\|/release]`, `ipv6config [/all\|auto\|off]`, `ping [-n N] [-l SIZE] [-t]`, `tracert`, `nslookup`, `telnet`, `ssh`, `ftp`, `netstat`, `dir`, `arp -a\|-d`, `hostname`, `cls`, with aligned help. |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Loopbacks; OSPF as the courses teach it; floating static routes; routers as IPv6 clients; DHCP snooping and ARP inspection; edge protection and errdisable recovery; STP tuning and EtherChannel views; small switch commands; ACL editing; DHCP server details; network management; HSRP v2 and tracking; console and PC prompt fixes. Section figures aligned with ROADMAP.md; the multilayer, OSPFv2, NDP, firewall and host-shell rows corrected. |
| 2026-10-30 | minor | EIGRP timers, packets and load balancing; OSPF equal-cost paths; tunnels over learned routes; IKE on the wire; QoS queuing. |
| 2026-10-30 | minor | QoS: class maps, policy maps, DSCP marking, policing. |
| 2026-10-30 | minor | SNMP traps and SNMPv3 (no privacy); mail relay between domains with bounces. |
| 2026-10-30 | minor | EIGRP; GRE tunnels; IPsec site-to-site. |
| 2026-10-30 | minor | Spanning-tree listening/learning timers and `spanning-tree mode rapid-pvst`. |
| 2026-10-30 | minor | NTP, syslog and TFTP. |
| 2026-10-30 | minor | DHCPv6 and router advertisement flags. |
| 2026-10-30 | minor | OSPF depth: cost, priority and DR/BDR, reference bandwidth, inter-area routes, interface and process views, multilayer switches. |
| 2026-10-30 | minor | access-class, IPv6 ACLs, NAT for ICMP errors. |
| 2026-10-30 | minor | PPP PAP and CHAP authentication. |
| 2026-10-30 | minor | DTP. |
| 2026-10-30 | minor | HSRP. |
| 2026-10-30 | minor | Logging buffer and console messages; `debug ip icmp`. |
| 2026-10-30 | minor | BPDUs, OSPF hellos and RIP updates on the wire, so Simulation mode and captures show them. |
| 2026-10-30 | minor | SNMP agent and the MIB Browser. |
| 2026-10-30 | minor | TACACS+. |
| 2026-10-30 | minor | `ip address dhcp` on routers, routed ports and the home router's Internet port. |
| 2026-10-30 | minor | Email: SMTP, POP3, the server's EMAIL service and the PC Email app; TCP retries a SYN four times. |
| 2026-10-30 | minor | CCNA 1 lab commands: output filters, password rules, login blocking, privilege 15 users, speed/duplex/mdix (a speed mismatch takes the link down), many `show` commands, `delete vlan.dat`; VLANs live in the VLAN database, not the configuration (except VTP transparent). |
| 2026-10-30 | minor | `debug ip packet`, `debug ip nat`, `debug ip dhcp server events`. |
| 2026-10-30 | minor | FTP. |
| 2026-10-30 | minor | AAA with RADIUS. |
| 2026-10-30 | minor | OSPFv3. |
| 2026-10-30 | minor | DHCPv6 relay. |
| 2026-10-30 | minor | IPv6 on subinterfaces, VLAN interfaces and multilayer switches. |
| 2026-10-30 | minor | OSPF stub and totally stubby areas, `area range`; RIP on multilayer switches. |
| 2026-10-30 | minor | OSPF database exchange (DBD, LSR, LSU, LSAck) on the wire. |
| 2026-10-30 | minor | LACPDUs on the wire. |
| 2026-10-30 | minor | `debug ip rip`, `debug ip ospf`. |
| 2026-10-30 | minor | LLDP with frames on the wire, `show processes`, `show memory`. |
| 2026-10-30 | minor | Console: CDP, clock, flash, users, terminal, show ip interface, show interfaces status and switchport, show running-config interface, richer show version. |
| 2026-10-30 | major | Created: every protocol and console area with its standard, status and estimate. |
