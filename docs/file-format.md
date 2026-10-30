# The `.nlcraft` lab file

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (scenarios, automation, router security, FHRP, EIGRP for IPv6, switch and port additions, power, clouds, SIP, IoT timers and rules, modules)

A lab is one UTF-8 JSON file. It is the serde form of `netlabcraft_model::Topology`
(`crates/model/src/lib.rs`), wrapped with a format tag and version (`crates/model/src/file.rs`):

```json
{"format": "netlabcraft", "version": 1, "topology": {"devices": [...], "links": [...], "next_id": 7, "notes": "...", "annotations": [...]}}
```

Fields that hold their default value are left out when saving, so a small lab stays small and an
older file without a newer field still opens.

## Topology

| Field | Holds |
|---|---|
| `devices` | The devices (below). |
| `links` | Cables: `a` and `b`, each `{"device": id, "port": index}`, and `cable` (`straight-through`, `cross-over`, `fiber`, `serial-dce`, `serial-dte`, `coaxial`, `phone`; left out when automatic). On a serial cable `a` is the DCE end. |
| `consoles` | Console cables: `{"pc": id, "device": id}` (the PC's RS-232 port to the device's Console port); left out when there are none. |
| `next_id` | The next device id (ids are never reused). |
| `notes` | The lab's notes text. |
| `annotations` | Notes and drawings on the workspace: text, rectangle, ellipse, line, each with position and size. |
| `activity` | A graded activity: `title`, `instructions`, `checks` (each `label`, `points`, `type`: `config`, `config-hashed`, `ping`, `host-address`, `output`, with its fields), `hidden`, `salt`. |
| `scenarios` | Simulation scenarios: `list` (each `description` and `pdus`) and `current`, the one in use. A PDU holds `num`, `source` (device id), `port`, `destination` (address or name) and `target`, `app` (`ping`, `dns`, `http`, `telnet`, `ssh`, `ftp`, `smtp`, `pop3`, `dhcp`, `tftp`, `snmp`, `arp`, `udp`, `tcp`), `simple`, `src_port`, `dst_port`, `ttl`, `tos`, `seq`, `size`, `text` (DNS name, page, file or community), `period_s` (periodic) and `start_s`, and what happened last: `status`, `last_s`, `fired`, `note`, `next_us`. Left out while the only scenario is empty. |

## Device

| Field | Holds |
|---|---|
| `id`, `kind`, `model`, `name`, `label`, `x`, `y` | Identity (`kind`: `router`, `switch`, `hub`, `pc`, `laptop`, `server`, `printer`, `phone`, `iot`; the Home Gateway is a `switch`, the Cloud and the modems are `hub`s; `model`: catalogue id such as `Switch-ML-24`, empty in old files means the kind's default), workspace label and position. |
| `ports` | The ports (below), in catalogue order. |
| `gateway`, `gateway6`, `dns`, `dhcp_client` | Hosts: default gateways, DNS server, whether the address came from DHCP. |
| `routes`, `routes6` | Static IPv4 and IPv6 routes; `distance` when one was typed (a floating static). |
| `loopbacks` | Routers and switches: loopback interfaces, sorted by number. |
| `ip_routing`, `ipv6_routing` | Multilayer switches: `ip routing`; routers: `ipv6 unicast-routing`. |
| `vlans`, `vtp`, `stp_priority`, `svis` | Switches: VLAN database (id → name), VTP mode/domain/password/version/revision, spanning-tree priorities per VLAN, VLAN interfaces. |
| `dhcp_pools`, `dhcp_excluded` | Routers: DHCP server pools (with `lease_secs` and `domain` when set) and excluded ranges. |
| `ospf`, `rip`, `eigrp` | Routing protocol settings. |
| `eigrp6` | `ipv6 router eigrp N`: `asn`, `router_id`, `passive`, `shutdown`, `maximum_paths`. |
| `fhrp` | VRRP and GLBP groups by interface name: `vrrp` (each `group`, `ip`, `priority`, `preempt`), `glbp` (each `group`, `ip`, `priority`, `preempt`, `balance`), and `vrrp_v3` (`fhrp version vrrp v3`). |
| `crypto` | IPsec: ISAKMP policies and keys, transform sets, crypto maps (an entry's `ikev2_profile` makes it negotiate with IKEv2), and `ikev2`: `proposals` (`encryption`, `integrity`, `group`), `policies` (`proposals`, `local`), `keyrings` (`peers` with `address`, `local_key`, `remote_key`) and `profiles` (`remote`, `local_id`, `auth_local`, `auth_remote`, `keyring`). Pre-shared keys are stored as typed. |
| `security` | Routers: the zone-based firewall (`zones`, `members` interface → zone, inspect `classes`, `policies`, zone `pairs`), classic inspection (`inspect_rules`, `inspect_applied` per interface with `inbound`/`outbound`), `ip_mtu` and `adjust_mss` per interface. |
| `automation` | Routers: `http`, `https`, `http_auth_local`, `restconf`, `netconf`. Hosts: the Programming app's `saved` requests (`name`, `method`, `url`, `headers`, `body`) and, on a controller, the `managed` routers (`address`, `user`, `password`). **The managed routers' passwords are stored in clear**, so a lab that holds them should be shared with care. |
| `acls`, `nat` | Access lists; NAT rules and pools. |
| `access` | Passwords (enable secret is stored hashed), console and vty lines, local users, banner, SSH settings. |
| `services` | Servers: HTTP files, DNS records, DHCP pools. |
| `startup` | The saved configuration (`copy running-config startup-config`), as console lines. |
| `l2` | Switches: DHCP snooping and ARP inspection (VLANs, option 82, validation), PortFast and BPDU guard defaults, errdisable recovery causes and interval, EtherChannel load balancing, the SDM template, `mls_qos`, `loopguard_default`; routers: `relay_trust_all` (`ip dhcp relay information trust-all`). |
| `modules` | Modular routers: the card in each bay (`"0/1": "Card-4SW"`; the cards are `Card-2S`, `Card-4S`, `Card-1GE`, `Card-2GE`, `Card-1FE`, `Card-1GF`, `Card-4SW`). |
| `adapter` | IP phones and lightweight APs: whether the power adapter is plugged in. Left out on every other device; a powered device in an older file has none and counts as plugged in. |
| `bridges` | Clouds: the port pairs joined to each other, such as `["Coaxial0", "Phone0"]`. |
| `voice` | Routers: `telephony-service`, `ephone-dn`s and `ephone`s, option 150 per pool, and for SIP `register` (`voice register global`: `cme`, `source`, `port`, `max_dn`, `max_pool`), `reg_dns` and `pools` (`tag`, `mac`, `dn`). Phones: the TFTP server DHCP gave, `sip` (signal with SIP) and `sip_server`. |
| `iot` | Smart things: `server`, `value` (a sensor's reading) and `interval` (report every N seconds of simulated time; 0 only on change). Servers and home gateways: `service` and `rules`, each with `conditions` (`sensor`, `op` `>`/`<`/`=`, `value`), `any` (joined with OR, else AND), `actuator` and `on` (what the rule does when it holds; true when left out). Older rules keep `sensor` and `above` and still read. |
| `mgmt` | Routers and switches: timestamps, logging levels, NTP master, keys and trusted keys, `boot system` lines, history size, HSRP version 2 interfaces. |
| `nvram` | Routers and switches: the configuration register and files copied into flash (kept across reloads). |

## Port

| Field | Holds |
|---|---|
| `name`, `mac`, `shutdown`, `description` | Identity and state. |
| `ip`, `helpers`, `access`, `nat` | IPv4 address (`"10.0.0.1/24"`), DHCP relay targets, applied access lists, NAT inside/outside. |
| `ipv6` | IPv6: `enabled`, `addresses`, `link_local`, `autoconfig`, `eigrp` (the EIGRP for IPv6 AS the interface runs). |
| `poe_never` | PoE switch ports: `power inline never`. |
| `subifs` | Router subinterfaces: number, 802.1Q VLAN, address, state, relay, ACLs, NAT. |
| `switchport`, `portfast`, `port_security`, `channel`, `routed`, `err_disabled` | Switches: mode, access/native VLAN, allowed list (`"1-10,20"`); portfast; port security; EtherChannel group and mode; routed port; err-disabled state. |
| `serial` | Serial ports: `encapsulation` (`hdlc`, `ppp`) and `clock_rate`. |
| `l2` | Switch ports: snooping and ARP-inspection trust and rate limits, BPDU guard, root guard, link type, spanning-tree cost and port priority per VLAN, LACP port priority, trunk encapsulation, the err-disable cause, `qos_trust` (`dscp` or `cos`), `loop_guard`, `bpdufilter` (true for enable, false for disable, left out when neither); router interfaces: `relay_trusted`. |

## Reading rules

Readers accept older versions, refuse newer ones, cap the size (`MAX_BYTES`, 64 MiB) and
sanitize everything (duplicate ids and names, dangling or duplicate cables, non-finite positions,
missing VLAN 1, oversized lists). Dynamic state (ARP and neighbour caches, MAC tables, leases,
events) is not saved.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | `scenarios`; `automation` (managed passwords stored in clear); `security`; `fhrp`; `eigrp6` and the port `ipv6.eigrp`; `crypto.ikev2`; switch and port `l2` additions; `modules` with `Card-4SW`; `adapter`, `bridges`, `poe_never`; `voice` SIP fields; `iot.interval` and rules with `conditions`, `any`, `on`; `coaxial` and `phone` cables; the `phone` and `iot` kinds listed. |
| 2026-10-30 | minor | `loopbacks`, route `distance`, DHCP pool lease and domain, switch and port `l2`, `mgmt`, `nvram`. |
| 2026-10-30 | minor | `links[].cable` and `consoles`. |
| 2026-10-30 | minor | `activity`. |
| 2026-10-30 | minor | Every field listed by table; IPv6, serial, EtherChannel, VTP, RIP, multilayer fields added since M0. |
| 2026-10-29 | major | Created. |
