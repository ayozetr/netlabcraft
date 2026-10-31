# Lab 2 — IPv4 and IPv6 on two LANs

**Course:** CCNA 1 (Introduction to Networks) · **Time:** 40 minutes

## Objectives

- Address a router's two LAN interfaces with IPv4 and IPv6 (global unicast and a chosen
  link-local address).
- Address one PC by hand and let the other configure itself with SLAAC.
- Verify both stacks with `show` commands, `ping` and `tracert`.

## Topology

```text
 PC1 ──Fa0/1── S1 ──G0/1── (G0/0) R1 (G0/1) ──G0/1── S2 ──Fa0/1── PC2
```

| Device | Interface | IPv4 | IPv6 | Link-local |
|---|---|---|---|---|
| R1 | G0/0 | 10.10.1.1/24 | 2001:db8:acad:1::1/64 | fe80::1 |
| R1 | G0/1 | 10.10.2.1/24 | 2001:db8:acad:2::1/64 | fe80::1 |
| PC1 | Fa0 | 10.10.1.10/24, gw 10.10.1.1 | 2001:db8:acad:1::10/64, gw fe80::1 | automatic |
| PC2 | Fa0 | 10.10.2.10/24, gw 10.10.2.1 | SLAAC (from R1's advertisements) | automatic |

R1 is a Router-2, S1 and S2 are Switch-24, PC1 and PC2 are PCs.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "S2", "b_port": "g0/1"}
link.add {"a": "PC1", "a_port": "fa0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "a_port": "fa0", "b": "S2", "b_port": "fa0/1"}
```

## Part 1 — Router interfaces

`ipv6 unicast-routing` makes R1 route IPv6 and send router advertisements, which PC2 needs
for SLAAC. The same link-local address can be used on every interface: it only has to be
unique on its own link.

```console R1
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ip address 10.10.1.1 255.255.255.0
ipv6 address 2001:db8:acad:1::1/64
ipv6 address fe80::1 link-local
no shutdown
interface g0/1
ip address 10.10.2.1 255.255.255.0
ipv6 address 2001:db8:acad:2::1/64
ipv6 address fe80::1 link-local
no shutdown
end
```

## Part 2 — Hosts

PC1 is addressed by hand for both stacks. Its IPv6 gateway is the router's link-local
address.

```console PC1
ipconfig 10.10.1.10 255.255.255.0 10.10.1.1
ipv6config 2001:db8:acad:1::10/64 fe80::1
```

PC2 takes its IPv4 address by hand and its IPv6 address from the router's advertisement
(the prefix plus an interface identifier it makes itself):

```console PC2
ipconfig 10.10.2.10 255.255.255.0 10.10.2.1
ipv6config auto
```

## Verification

```verify
R1# show ip interface brief
  expect: 10.10.1.1
  expect: 10.10.2.1
R1# show ipv6 interface brief
  expect: 2001:db8:acad:1::1/64
  expect: fe80::1 (link-local)
R1# show ipv6 route
  expect: 2001:db8:acad:2::/64
PC2> ipconfig
  expect: 2001:db8:acad:2:
```

Both stacks reach across the router:

```verify
ping PC1 10.10.2.10 => success
ping PC1 2001:db8:acad:2::1 => success
ping PC2 2001:db8:acad:1::10 => success
PC1> tracert 10.10.2.10
  expect: 10.10.1.1
  expect: 10.10.2.10
```

## Reflection

1. Which part of PC2's IPv6 address came from R1, and which part did PC2 make itself?
2. Why does PC1 use `fe80::1` as its IPv6 gateway rather than `2001:db8:acad:1::1`?
3. In `tracert`, why is the first hop R1's G0/0 address and not its G0/1 address?
