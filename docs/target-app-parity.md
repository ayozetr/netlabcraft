# Target-app parity

_Updated 2026-10-29. Authoritative assessment against Cisco Packet Tracer as a CCNA teaching tool, by black-box observation and public documentation only (see AGENTS.md, clean room)._

| Area | Reference app | PacketCraft | Notes |
|---|---|---|---|
| Logical workspace (place, move, cable devices) | yes | yes | Port chooser per cable; link lights; zoom/pan; grid snap. |
| Physical workspace (buildings, racks) | yes | no | Low priority. |
| Device catalogue | many models | 4 generic kinds | Generic on purpose; modular slots later. |
| Cable types (straight, cross, fibre, serial, console) | yes | one Ethernet cable, auto-MDIX | Serial (PPP/HDLC) planned with WAN work. |
| Switch MAC learning, VLANs, trunks | yes | yes | |
| STP / RSTP | yes | per-VLAN tree, computed instantly | No BPDU frames or timers yet. |
| Port security | yes | yes | sticky, static, three violation modes, err-disabled |
| EtherChannel (LACP, PAgP, static) | yes | yes | computed negotiation; members spread traffic by address |
| VTP | yes | yes | computed; no pruning or version 3 |
| DTP | yes | no | |
| IPv4 static routing, router-on-a-stick | yes | yes | |
| Switch VLAN interface (management SVI, default gateway) | yes | yes | |
| Layer-3 switch (`ip routing`) | yes | no | |
| OSPF | yes | single area, computed convergence | No hello/LSA frames yet. |
| RIP, EIGRP, BGP | yes | no | |
| IPv6 | yes | no | |
| DHCP server/relay | yes | server on routers, relay, clients on hosts | No DHCP service on servers yet. |
| DNS, HTTP (server and browser) | yes | yes | HTTP over simulated TCP; no HTTPS. |
| FTP, TFTP, email, syslog, NTP, AAA | yes | no | |
| ACLs, NAT/PAT | yes | yes | Standard/extended, numbered/named; static, pool, PAT. |
| Passwords / `enable secret` / lines / banner | yes | yes | Real password prompts on the console. |
| SSH / Telnet | yes | no | |
| Router/switch console (modes, abbreviations, `?`, Tab) | yes | yes | Our own messages and table layouts. |
| Running/startup config, `write`, `reload` | yes | yes | |
| PC desktop (IP config, command prompt, web browser) | yes | yes | No email client, terminal or Telnet/SSH client yet. |
| Simulation mode: event list, step, replay | yes | yes | Plus drop reasons, hex dump. |
| PDU details (per layer) | yes | yes | Decoded from real bytes. |
| Packet capture export (pcap) | no | **yes** | Opens in any analyser. |
| Activity wizard / grading | yes | no | Planned as checks over the lab (`docs/gaps.md`). |
| Scripting / automation | limited | **full** | CLI and MCP reach every command. |
| Open file format | no | **yes** | JSON `.pcraft`. |
| Platforms | desktop | desktop + browser | |
| Licence | proprietary, account required | MIT/Apache-2.0, no account | |
