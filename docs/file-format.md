# The `.pcraft` lab file

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (cable kinds)

A lab is one UTF-8 JSON file. It is the serde form of `packetcraft_model::Topology`
(`crates/model/src/lib.rs`), wrapped with a format tag and version (`crates/model/src/file.rs`):

```json
{"format": "packetcraft", "version": 1, "topology": {"devices": [...], "links": [...], "next_id": 7, "notes": "...", "annotations": [...]}}
```

Fields that hold their default value are left out when saving, so a small lab stays small and an
older file without a newer field still opens.

## Topology

| Field | Holds |
|---|---|
| `devices` | The devices (below). |
| `links` | Cables: `a` and `b`, each `{"device": id, "port": index}`, and `cable` (`straight-through`, `cross-over`, `fiber`, `serial-dce`, `serial-dte`; left out when automatic). On a serial cable `a` is the DCE end. |
| `consoles` | Console cables: `{"pc": id, "device": id}` (the PC's RS-232 port to the device's Console port); left out when there are none. |
| `next_id` | The next device id (ids are never reused). |
| `notes` | The lab's notes text. |
| `annotations` | Notes and drawings on the workspace: text, rectangle, ellipse, line, each with position and size. |
| `activity` | A graded activity: `title`, `instructions`, `checks` (each `label`, `points`, `type`: `config`, `config-hashed`, `ping`, `host-address`, `output`, with its fields), `hidden`, `salt`. |

## Device

| Field | Holds |
|---|---|
| `id`, `kind`, `model`, `name`, `label`, `x`, `y` | Identity (`kind`: `router`, `switch`, `hub`, `pc`, `laptop`, `server`, `printer`; `model`: catalogue id such as `Switch-ML-24`, empty in old files means the kind's default), workspace label and position. |
| `ports` | The ports (below), in catalogue order. |
| `gateway`, `gateway6`, `dns`, `dhcp_client` | Hosts: default gateways, DNS server, whether the address came from DHCP. |
| `routes`, `routes6` | Static IPv4 and IPv6 routes. |
| `ip_routing`, `ipv6_routing` | Multilayer switches: `ip routing`; routers: `ipv6 unicast-routing`. |
| `vlans`, `vtp`, `stp_priority`, `svis` | Switches: VLAN database (id → name), VTP mode/domain/password/version/revision, spanning-tree priorities per VLAN, VLAN interfaces. |
| `dhcp_pools`, `dhcp_excluded` | Routers: DHCP server pools and excluded ranges. |
| `ospf`, `rip` | Routing protocol settings. |
| `acls`, `nat` | Access lists; NAT rules and pools. |
| `access` | Passwords (enable secret is stored hashed), console and vty lines, local users, banner, SSH settings. |
| `services` | Servers: HTTP files, DNS records, DHCP pools. |
| `startup` | The saved configuration (`copy running-config startup-config`), as console lines. |

## Port

| Field | Holds |
|---|---|
| `name`, `mac`, `shutdown`, `description` | Identity and state. |
| `ip`, `helpers`, `access`, `nat` | IPv4 address (`"10.0.0.1/24"`), DHCP relay targets, applied access lists, NAT inside/outside. |
| `ipv6` | IPv6: `enabled`, `addresses`, `link_local`, `autoconfig`. |
| `subifs` | Router subinterfaces: number, 802.1Q VLAN, address, state, relay, ACLs, NAT. |
| `switchport`, `portfast`, `port_security`, `channel`, `routed`, `err_disabled` | Switches: mode, access/native VLAN, allowed list (`"1-10,20"`); portfast; port security; EtherChannel group and mode; routed port; err-disabled state. |
| `serial` | Serial ports: `encapsulation` (`hdlc`, `ppp`) and `clock_rate`. |

## Reading rules

Readers accept older versions, refuse newer ones, cap the size (`MAX_BYTES`, 64 MiB) and
sanitize everything (duplicate ids and names, dangling or duplicate cables, non-finite positions,
missing VLAN 1, oversized lists). Dynamic state (ARP and neighbour caches, MAC tables, leases,
events) is not saved.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | `links[].cable` and `consoles`. |
| 2026-10-30 | minor | `activity`. |
| 2026-10-30 | minor | Every field listed by table; IPv6, serial, EtherChannel, VTP, RIP, multilayer fields added since M0. |
| 2026-10-29 | major | Created. |
