# Gaps (ranked work list)

_Updated 2026-10-29. Pick from the top; each item lands with tests and doc updates._

1. ~~DHCP~~ — done 2026-10-29 (router pools, exclusions, bindings, conflict detection, relay with `ip helper-address`, host `/renew`). Still to do: DHCP service on Server devices.
2. ~~Spanning tree~~ — done 2026-10-29 (PVST-style per-VLAN trees, roles and blocking, priorities, `root primary/secondary`, portfast, `show spanning-tree`, amber lights). Still to do: BPDUs in the event list, convergence timers, RSTP proposal/agreement.
3. ~~Switch VLAN interfaces~~ — done 2026-10-29 (`interface vlan N`, `ip default-gateway`). Still to do: layer-3 switching (`ip routing` on multilayer switches).
4. ~~OSPF single area~~ — done 2026-10-29 (computed convergence: neighbours, SPF, passive interfaces, router id, default-information originate, `show ip ospf neighbor`, `show ip protocols`). Still to do: hello/LSA packets in the event list, multi-area (IA routes), `ip ospf cost`, RIP.
5. **Unsaved-changes prompt in the web build**, and recent files on desktop.
6. ~~ACLs and NAT~~ — done 2026-10-29 (numbered and named standard/extended ACLs with counters; static NAT, pools, PAT with interface overload). Still to do: `access-class` on vty lines, NAT for ICMP errors (traceroute through NAT).
7. **Remote access**: passwords are done (`enable secret/password`, console and vty `password`/`login`, `banner motd`, `service password-encryption`); still to do: `username`/`login local`, Telnet/SSH between devices.
8. **Activity checks**: a lab can carry expected results (pings that must succeed, config lines that must exist) and a score.
9. **Serial WAN links** (PPP/HDLC, clock rate) and more device models (modular slots).
10. ~~IPv6~~ — done 2026-10-29 (global, EUI-64 and link-local addresses, neighbour discovery, router solicitations/advertisements and host autoconfiguration, static routes with link-local next hops, `ipv6 unicast-routing`, ICMPv6 echo/errors, ping and traceroute, `show ipv6 interface brief|route|neighbors`, PC IPv6 configuration, ipv6 sample lab). Still to do: OSPFv3, DHCPv6, IPv6 on subinterfaces and switch VLAN interfaces, IPv6 ACLs.
11. **Translations** through `craft-i18n`.
12. **Packaging**: desktop icons, installers, release workflow like the other Craft apps.
13. ~~Services on Server devices~~ — done 2026-10-29 (HTTP over simulated TCP with a file manager, DNS A records, DHCP pools on servers; Web Browser on the PC Desktop; `nslookup`, names in `ping`/`tracert`). Still to do: HTTPS, Email, FTP, TFTP, NTP, Syslog, AAA.
14. ~~Telnet/SSH between devices~~ — done 2026-10-29 (`telnet`/`ssh -l` from hosts and devices, `username`, `login local`, `transport input`, `crypto key generate rsa`, `ip ssh version`).
15. ~~VTP~~ — done 2026-10-29 (server/client/transparent, domain learned from the first advertisement, password, version 2 relaying, highest revision wins, client and extended-range restrictions, VLAN database kept across reloads, `show vtp status`, a vtp sample lab). Still to do: VTP pruning, version 3, DTP. EtherChannel done 2026-10-29 (`channel-group N mode on|active|passive|desirable|auto`, negotiation from both ends' modes, suspended members, `interface port-channel`, one spanning-tree port per bundle, `show etherchannel summary`, an etherchannel sample lab). Port security done 2026-10-29 (`maximum`, `violation shutdown|restrict|protect`, sticky and static addresses, err-disabled ports recovered with `shutdown`/`no shutdown`, `show port-security`).
