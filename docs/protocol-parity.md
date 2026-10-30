# Protocol and command parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (created) · **Target:** Cisco Packet Tracer 9.0 (as used in CCNA courses)

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
| Spanning tree (per VLAN) | IEEE 802.1D | 🟡 | Roles, priorities, `root primary/secondary`, portfast, amber lights; computed at once. No BPDU frames, timers or RSTP proposal/agreement: 10–16 h. |
| EtherChannel | IEEE 802.1AX (LACP); PAgP | ✅ | `on`, `active/passive`, `desirable/auto`; suspended members; one STP port per bundle; traffic spread by address; `interface port-channel`, `show etherchannel summary`. No LACP frames. |
| VTP | — | 🟡 | Server/client/transparent, domain, password, versions 1/2, revisions. No pruning, version 3 or advertisement frames: 4–6 h. |
| DTP | — | ❌ | `switchport mode dynamic auto/desirable`, `switchport nonegotiate`: 4–6 h. |
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
| Multilayer switching | — | ✅ | `ip routing`, VLAN interfaces, routed ports (`no switchport`), static routes. No OSPF/RIP on switches yet: 4–6 h. |
| RIP | RFC 1058, RFC 2453 | 🟡 | Versions 1/2 (both classless here), classful `network`, passive interfaces, split horizon, 15 hops, default origination. No update packets or auto-summary: 6–10 h. |
| OSPFv2 | RFC 2328 | 🟡 | Single area: neighbours, SPF, router id, passive interfaces, default origination, cost 1 on Ethernet and 64 on serial. No hello/LSA frames, multi-area, `ip ospf cost`, DR/BDR display: 20–30 h. |
| EIGRP | RFC 7868 | ❌ | 15–25 h. |
| IPv6 addressing | RFC 8200, RFC 4291 | ✅ | Global, EUI-64 and link-local addresses, `ipv6 enable`. |
| NDP and SLAAC | RFC 4861, RFC 4862 | ✅ | Neighbour solicitation/advertisement, router solicitation/advertisement, host autoconfiguration. No DAD or RA flags (M/O). |
| ICMPv6 | RFC 4443 | ✅ | Echo, time exceeded, unreachable; ping and traceroute. |
| IPv6 static routes | — | ✅ | Next hop, interface, or link-local next hop with its interface. |
| OSPFv3, DHCPv6 | RFC 5340, RFC 8415 | ❌ | 20–30 h. |
| IPv6 on subinterfaces and VLAN interfaces | — | ❌ | 4–6 h. |
| FHRP (HSRP) | RFC 2281 | ❌ | 12–18 h. |

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
| FTP, TFTP | RFC 959, RFC 1350 | ❌ | Including `copy running-config tftp:`: 10–16 h. |
| Email (SMTP/POP3) | RFC 5321, RFC 1939 | ❌ | 10–15 h. |
| NTP, syslog, SNMP, AAA/RADIUS/TACACS+ | RFC 5905, 5424, 3416, 2865 | ❌ | 20–30 h. |

## Security

**Security: ~60% ready, 35–55 h** (estimated).

| Feature | Status | Notes and gap |
|---|---|---|
| Standard and extended ACLs, numbered and named | ✅ | `ip access-group in/out`, match counters, implicit deny, TCP/UDP ports. |
| `access-class` on vty lines | ❌ | 2–3 h. |
| IPv6 ACLs | ❌ | 6–10 h. |
| NAT: static, pool, PAT (overload) | ✅ | ICMP, UDP and TCP; `show ip nat translations`. No NAT for ICMP errors (traceroute through NAT): 3–5 h. |
| Passwords and lines | ✅ | `enable secret/password`, console and vty passwords with real prompts, `banner motd`, `service password-encryption`. |
| PPP authentication (PAP/CHAP) | ❌ | 6–10 h. |
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
| `show` coverage | 🟡 | Interfaces, IP/IPv6 brief and routes, ARP, MAC table, VLANs, trunks, STP, EtherChannel, VTP, port security, OSPF, RIP, DHCP, NAT, ACLs, SSH, controllers. Missing: `show version`, `show cdp neighbors`, `show flash`, `show clock`, `show users` and more: 15–25 h. |
| CDP / LLDP | ❌ | Discovery protocol and `show cdp neighbors`: 6–10 h. |
| `terminal`, `debug`, `clock set` | ❌ | 6–10 h. |
| Host shell (PC Command Prompt) | 🟡 | `ipconfig`, `ipv6config`, `ping`, `tracert`, `nslookup`, `telnet`, `ssh`, `arp`. No `netstat`, `ftp`. |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | major | Created: every protocol and console area with its standard, status and estimate. |
