# Lab 3 — Subnet a small company with VLSM

**Course:** CCNA 1 (Introduction to Networks) · **Time:** 45 minutes

## Scenario

A small company has been given 192.168.50.0/24. It needs:

| Network | Hosts needed |
|---|---|
| Sales LAN | 100 |
| IT LAN | 50 |
| Admin LAN | 20 |
| R1–R2 link | 2 |

## Objectives

- Divide the block with variable-length masks, largest network first, wasting as little as
  possible.
- Address two routers and three LANs from the plan.
- Join the routers with static routes and check every LAN reaches every other.

## The plan

Work it out before reading on: the smallest block that fits 100 hosts is a /25 (126 usable),
50 hosts a /26 (62), 20 hosts a /27 (30) and a point-to-point link a /30 (2).

| Network | Block | Mask | Usable range | Gateway |
|---|---|---|---|---|
| Sales | 192.168.50.0/25 | 255.255.255.128 | .1 – .126 | .1 |
| IT | 192.168.50.128/26 | 255.255.255.192 | .129 – .190 | .129 |
| Admin | 192.168.50.192/27 | 255.255.255.224 | .193 – .222 | .193 |
| R1–R2 | 192.168.50.224/30 | 255.255.255.252 | .225 – .226 | — |

192.168.50.228 – .255 is left for growth.

## Topology

```text
 Sales PC1 ── S1 ──G0/0 ┐
                        R1 ──G0/2──────G0/0── R2 ──G0/1── S3 ── Admin PC3
 IT PC2 ───── S2 ──G0/1 ┘
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-3 | G0/0 | 192.168.50.1/25 |
| R1 | | G0/1 | 192.168.50.129/26 |
| R1 | | G0/2 | 192.168.50.225/30 |
| R2 | Router-2 | G0/0 | 192.168.50.226/30 |
| R2 | | G0/1 | 192.168.50.193/27 |
| PC1 | PC | Fa0 | 192.168.50.10/25 |
| PC2 | PC | Fa0 | 192.168.50.140/26 |
| PC3 | PC | Fa0 | 192.168.50.200/27 |

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "Switch-24", "name": "S3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "S2", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/2", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "S3", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S2", "b_port": "fa0/1"}
link.add {"a": "PC3", "b": "S3", "b_port": "fa0/1"}
```

## Part 1 — Routers

R1 knows Sales and IT directly; it reaches Admin through R2. R2 sends everything it doesn't
know to R1 with a default route.

```console R1
enable
configure terminal
interface g0/0
description Sales
ip address 192.168.50.1 255.255.255.128
no shutdown
interface g0/1
description IT
ip address 192.168.50.129 255.255.255.192
no shutdown
interface g0/2
description Link to R2
ip address 192.168.50.225 255.255.255.252
no shutdown
exit
ip route 192.168.50.192 255.255.255.224 192.168.50.226
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 192.168.50.226 255.255.255.252
no shutdown
interface g0/1
description Admin
ip address 192.168.50.193 255.255.255.224
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 192.168.50.225
end
```

## Part 2 — Hosts

```console PC1
ipconfig 192.168.50.10 255.255.255.128 192.168.50.1
```

```console PC2
ipconfig 192.168.50.140 255.255.255.192 192.168.50.129
```

```console PC3
ipconfig 192.168.50.200 255.255.255.224 192.168.50.193
```

## Verification

Each subnet shows with its own prefix length, and the static and default routes are in place:

```verify
R1# show ip route
  expect: 192.168.50.0/25
  expect: 192.168.50.128/26
  expect: 192.168.50.224/30
  expect: S    192.168.50.192/27    via 192.168.50.226
R2# show ip route
  expect: S*
  expect: 192.168.50.192/27
ping PC1 192.168.50.140 => success
ping PC1 192.168.50.200 => success
ping PC2 192.168.50.200 => success
ping PC3 192.168.50.10 => success
```

## Reflection

1. How many addresses does the plan leave for growth, and what is the largest subnet that
   could still be made from them?
2. What would go wrong if Admin were given 192.168.50.160/27?
3. Why is a /30 used for the link between the routers?
