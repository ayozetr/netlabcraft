# Protocol and command parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (OSPF database exchange on the wire) · **Target:** Cisco Packet Tracer 9.0 (as used in CCNA courses)

What the simulated devices do, protocol by protocol, against the reference app. Part of
[target-app-parity.md](target-app-parity.md). Behaviour comes from the standards named in each
row and from black-box observation; console wording and table layouts are our own. Every
console command is listed in [`command-reference.md`](command-reference.md) (generated).

**Status:** ✅ works · 🟡 partly · ❌ not yet.

## Layer 2

**Layer 2: ~75% ready, 25–40 h** (estimated).

| Feature | Standard | Status | Notes and gap |
|---|---|---|---|
| Ethernet II and 802.1Q frames | IEEE 802.3, 802.1Q | ✅ | Real encoded frames, padded to 64 bytes. |
| MAC learning and aging | IEEE 802.1D | ✅ | Per (VLAN, MAC); flushed when the topology changes. `show mac address-table`, `clear mac address-table`. |
| VLANs, access and trunk ports | IEEE 802.1Q | ✅ | Native VLAN, allowed lists, `show vlan brief`, `show interfaces trunk`. |
| Spanning tree (per VLAN) | IEEE 802.1D | 🟡 | Roles, priorities, `root primary/secondary`, portfast, amber lights; computed at once; configuration BPDUs (IEEE format, tagged per VLAN on trunks) every 2 s from designated ports; a port that starts forwarding listens 15 s and learns 15 s (LIS/LRN in `show spanning-tree`, amber light) unless portfast or `spanning-tree mode rapid-pvst`. No max-age wait or RSTP proposal/agreement frames: 4–6 h. |
| EtherChannel | IEEE 802.1AX (LACP); PAgP | ✅ | `on`, `active/passive`, `desirable/auto`; suspended members; one STP port per bundle; traffic spread by address; `interface port-channel`, `show etherchannel summary`; LACPDUs every 30 s (actor and partner, sync state) in Simulation mode and captures. PAgP has no frames: its format isn't public. |
| VTP | — | 🟡 | Server/client/transparent, domain, password, versions 1/2, revisions. No pruning, version 3 or advertisement frames: 4–6 h. |
| DTP | — | ✅ | `switchport mode dynamic auto/desirable`, `switchport nonegotiate`; the working mode is negotiated from both ends and shown in `show interfaces X switchport`, `show interfaces trunk`, `show vlan brief`. Ports default to access (real access switches default to dynamic auto). No DTP frames. |
| Port security | — | ✅ | Maximum, violation shutdown/restrict/protect, sticky and static addresses, err-disabled recovery, `show port-security`. No `errdisable recovery` timer. |
| Hubs | IEEE 802.3 | ✅ | Repeat every frame. |
| Switch management (SVI, default gateway) | — | ✅ | `interface vlan N`, `ip default-gateway`. |

## Layer 3

**Layer 3: ~55% ready, 70–110 h** (estimated).

| Feature | Standard | Status | Notes and gap |
|---|---|---|---|
| IPv4, ARP, ICMP | RFC 791, 826, 792 | ✅ | TTL, time exceeded, unreachable (net, host, port, admin), proxy ARP. Routers drop the packet that starts ARP, as real ones do. |
| Static routes | — | ✅ | Next hop or exit interface, default route, longest prefix match. |
| Router-on-a-stick | IEEE 802.1Q | ✅ | Subinterfaces with `encapsulation dot1Q`. |
| Multilayer switching | — | ✅ | `ip routing`, VLAN interfaces, routed ports (`no switchport`), static routes, OSPF. No RIP on switches: 2–3 h. |
| RIP | RFC 1058, RFC 2453 | 🟡 | Versions 1/2 (both classless here), classful `network`, passive interfaces, split horizon, 15 hops, default origination. RIP responses every 30 s (multicast for v2, broadcast for v1) with split horizon. No triggered updates or auto-summary: 4–6 h. |
| OSPFv2 | RFC 2328 | 🟡 | Single area: neighbours, SPF, router id, passive interfaces, default origination, cost 1 on Ethernet and 64 on serial. Hellos every 10 s; `ip ospf cost`, `ip ospf priority` with DR/BDR election (priority 0 never DR, none on serial links), `auto-cost reference-bandwidth`, several areas with inter-area routes shown as `O IA`, `show ip ospf`, `show ip ospf interface [brief]`, DR/BDR in `show ip ospf neighbor`; also on multilayer switches. When neighbours meet, each sends its database description, a link-state request, a link-state update with its router-LSA (with a valid Fletcher checksum) and an acknowledgement, visible in Simulation mode and captures. No area types (stub, NSSA), summarisation, or network/summary LSAs on the wire: 8–12 h. |
| EIGRP | RFC 7868 | ❌ | 15–25 h. |
| IPv6 addressing | RFC 8200, RFC 4291 | ✅ | Global, EUI-64 and link-local addresses, `ipv6 enable`. |
| NDP and SLAAC | RFC 4861, RFC 4862 | ✅ | Neighbour solicitation/advertisement, router solicitation/advertisement, host autoconfiguration. No DAD or RA flags (M/O). |
| ICMPv6 | RFC 4443 | ✅ | Echo, time exceeded, unreachable; ping and traceroute. |
| IPv6 static routes | — | ✅ | Next hop, interface, or link-local next hop with its interface. |
| DHCPv6 | RFC 8415 | ✅ | `ipv6 dhcp pool` (address prefix, DNS, domain), `ipv6 dhcp server`, `ipv6 nd managed-config-flag` / `other-config-flag`; hosts follow the router: SLAAC, SLAAC with stateless DHCPv6, or stateful DHCPv6 (Solicit, Advertise, Request, Reply on the wire); `show ipv6 dhcp pool/binding`. No DHCPv6 relay. |
| OSPFv3 | RFC 5340 | ❌ | 12–18 h. |
| IPv6 on subinterfaces and VLAN interfaces | — | ❌ | 4–6 h. |
| FHRP (HSRP) | RFC 2281 | ✅ | `standby N ip/priority/preempt` on router interfaces, subinterfaces and multilayer VLAN interfaces; active answers ARP with the virtual MAC; hellos every 3 s; `show standby`. Election computed (no timers or coup/resign messages); no HSRP version 2, tracking or VRRP/GLBP: 6–10 h. |

## Services

**Services: ~40% ready, 40–60 h** (estimated).

| Feature | Standard | Status | Notes and gap |
|---|---|---|---|
| DHCPv4 server on routers | RFC 2131 | ✅ | Pools, exclusions, bindings, conflicts; `show ip dhcp binding/pool`. |
| DHCPv4 relay | RFC 2131 | ✅ | `ip helper-address`. |
| DHCPv4 on servers | RFC 2131 | ✅ | Services tab pools. |
| DHCP client on hosts | RFC 2131 | ✅ | `ipconfig /renew`, IP Configuration DHCP option. |
| DNS | RFC 1035 | ✅ | A records on servers; `nslookup`, names in ping/tracert/browser. |
| HTTP | RFC 9110 over TCP (RFC 9293) | ✅ | Server file manager, PC web browser. No HTTPS: 6–10 h. |
| Telnet and SSH | RFC 854, RFC 4253 | ✅ | Local users, `login local`, `transport input`, RSA keys, `ip ssh version`. The SSH session is simulated, not encrypted. |
| TFTP | RFC 1350 | ✅ | Server TFTP service with a file list; `copy running-config|startup-config tftp: <address> [file]`, `copy tftp: running-config|startup-config <address> <file>`; 512-byte blocks and acknowledgements on the wire (the server answers from port 69). |
| FTP | RFC 959 | ❌ | Server FTP users and the PC `ftp` client: 8–12 h. |
| Email (SMTP/POP3) | RFC 5321, RFC 1939 | ❌ | 10–15 h. |
| NTP | RFC 5905 | ✅ | Server NTP service; `ntp server` syncs at once and every 64 s; `show ntp status/associations`. No authentication or stratum chains. |
| Syslog | RFC 5424 | ✅ | `logging host` sends every log message to the server, whose SYSLOG page lists them. No severity filtering (`logging trap`). |
| SNMP, AAA/RADIUS/TACACS+ | RFC 3416, 2865 | ❌ | 14–22 h. |

## Security

**Security: ~60% ready, 35–55 h** (estimated).

| Feature | Status | Notes and gap |
|---|---|---|
| Standard and extended ACLs, numbered and named | ✅ | `ip access-group in/out`, match counters, implicit deny, TCP/UDP ports. |
| `access-class` on vty lines | ✅ | Telnet/SSH logins refused (TCP reset) when the list denies the source; counted in `show access-lists`. |
| IPv6 ACLs | ✅ | `ipv6 access-list` with `permit|deny ipv6|icmp|tcp|udp` and any/host/prefix addresses and ports, `ipv6 traffic-filter in|out`, neighbour discovery passes implicitly, `show ipv6 access-list` with matches; denied packets get an administratively-prohibited unreachable. |
| NAT: static, pool, PAT (overload) | ✅ | ICMP, UDP and TCP; `show ip nat translations`. ICMP errors about translated packets are translated back (traceroute works through PAT). |
| Passwords and lines | ✅ | `enable secret/password`, console and vty passwords with real prompts, `banner motd`, `service password-encryption`. |
| PPP authentication (PAP/CHAP) | ✅ | `ppp authentication chap|pap` (one or both, in order), `ppp pap sent-username`, checked against `username` entries (CHAP: both routers name each other with the same secret); failure keeps the line protocol down and `show interfaces` says why. No LCP/CHAP frames on the wire. |
| ASA firewall, zone-based firewall, VPN (GRE/IPsec) | ❌ | 40–60 h. |

## WAN

| Feature | Status | Notes and gap |
|---|---|---|
| Serial links (DCE/DTE, clock rate) | ✅ | The first end cabled is the DCE; no clock or mismatched framing keeps the line protocol down; `show controllers`. |
| HDLC and PPP framing | ✅ | On the wire; listed and captured as the equivalent Ethernet II frame (a capture has one link type). |
| Frame Relay, DSL/cable modems, cloud | ❌ | Low priority in current courses. |

## Console

**Device console: ~60% ready, 60–90 h** (estimated).

| Feature | Status | Notes and gap |
|---|---|---|
| Modes: user, privileged, global, interface, subinterface, range, VLAN, line, DHCP pool, router OSPF/RIP, ACL, VLAN interface | ✅ | Prompts follow the usual `name>`, `name#`, `name(config)#` convention. |
| Abbreviations, `?` help, Tab completion, history, `do` | ✅ | Ambiguity and unknown-word errors point at the word. |
| Running/startup config, `write`, `copy`, `erase`, `reload` | ✅ | Configuration round-trips through the same dispatcher (tested). |
| `show` coverage | 🟡 | Interfaces (also `status`, `X switchport`), IP/IPv6 interface and routes, ARP, MAC table, VLANs, trunks, STP, EtherChannel, VTP, port security, OSPF (also `interface`), RIP, DHCP, NAT, ACLs, SSH, controllers, CDP, LLDP, HSRP (`standby`), logging, version, flash, clock, users, history, processes, memory, `running-config interface X`. Missing: going through the CCNA 1 labs command by command: 4–8 h. |
| CDP | 🟡 | `cdp run`, `cdp enable`, `show cdp`, `show cdp neighbors [detail]`; computed, no advertisement frames (the CDP frame format is proprietary). |
| LLDP (IEEE 802.1AB) | ✅ | `lldp run`, `lldp transmit`/`receive` per port, `show lldp`, `show lldp neighbors [detail]`, `show lldp entry *`; real LLDP frames every 30 s in Simulation mode and captures. |
| `clock set`, `terminal`, `debug` | 🟡 | `clock set` and `show clock` (per device, advances with simulated time, lost on reload); `terminal length/width/monitor` accepted (output is never paged); `debug ip icmp`, `debug ip rip`, `debug ip ospf adj|events|hello` (the hellos heard), `undebug all`, `show debugging`. No packet-level `debug ip packet` or NAT/DHCP debugs: 3–5 h. |
| Logging and console messages | ✅ | A logging buffer per device (`show logging`, `clear logging`): link up/down, port-security shutdowns, configuration changes, debug output; the same messages appear on the device's console after the prompt, and go to syslog servers (`logging host`). |
| Host shell (PC Command Prompt) | 🟡 | `ipconfig`, `ipv6config`, `ping`, `tracert`, `nslookup`, `telnet`, `ssh`, `arp`. No `netstat`, `ftp`. |

## Revision history

| Date | Change | Summary |
|---|---|---|
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
| 2026-10-30 | minor | OSPF database exchange (DBD, LSR, LSU, LSAck) on the wire. |
| 2026-10-30 | minor | LACPDUs on the wire. |
| 2026-10-30 | minor | `debug ip rip`, `debug ip ospf`. |
| 2026-10-30 | minor | LLDP with frames on the wire, `show processes`, `show memory`. |
| 2026-10-30 | minor | Console: CDP, clock, flash, users, terminal, show ip interface, show interfaces status and switchport, show running-config interface, richer show version. |
| 2026-10-30 | major | Created: every protocol and console area with its standard, status and estimate. |
