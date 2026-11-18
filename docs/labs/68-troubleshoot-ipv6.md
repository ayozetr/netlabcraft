# Lab 68 — Troubleshooting: IPv6 that does not cross the routers

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Find three independent faults in a two-router IPv6 network by following one packet from the
  source: a router that does not forward IPv6, a static route whose next hop does not exist,
  and a host with the wrong default gateway.
- Read `show ipv6 route`, `show ipv6 route static`, `show ipv6 interface` and the host's own
  `ipv6config`, and know what each says about forwarding, routes and the first hop.
- Tell what the host can reach on its own link (neighbour discovery) from what it can reach
  through a gateway.

## Topology

```text
 PC1 ── (G0/0) R1 (G0/1) ──── (G0/0) R2 (G0/1) ── PC2
```

| Device | Model | Interface | IPv6 address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 2001:db8:68:1::1/64 |
| R1 | | G0/1 | 2001:db8:68:12::1/64 |
| R2 | Router-2 | G0/0 | 2001:db8:68:12::2/64 |
| R2 | | G0/1 | 2001:db8:68:2::1/64 |
| PC1 | PC | Fa0 | 2001:db8:68:1::10/64, gateway 2001:db8:68:1::1 |
| PC2 | PC | Fa0 | 2001:db8:68:2::10/64, gateway 2001:db8:68:2::1 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/1"}
```

## Part 1 — The network as it was configured

The hosts are configured from a sheet. PC2's sheet has the wrong gateway (the router's
address ends in 1, the sheet says 2). R1 has a static route to PC2's network and R2 has one back to PC1's;
R1's route names a next hop typed with a slip (…::3 for …::2). And one command is missing on R2.

```console PC1
ipv6config 2001:db8:68:1::10/64 2001:db8:68:1::1
```

```console PC2
ipv6config 2001:db8:68:2::10/64 2001:db8:68:2::2
```

```console R1
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ipv6 address 2001:db8:68:1::1/64
no shutdown
interface g0/1
ipv6 address 2001:db8:68:12::1/64
no shutdown
exit
ipv6 route 2001:db8:68:2::/64 2001:db8:68:12::3
end
```

```console R2
enable
configure terminal
interface g0/0
ipv6 address 2001:db8:68:12::2/64
no shutdown
interface g0/1
ipv6 address 2001:db8:68:2::1/64
no shutdown
exit
ipv6 route 2001:db8:68:1::/64 2001:db8:68:12::1
end
```

PC1 cannot reach PC2. Follow the packet from PC1: its gateway is R1, and R1 answers for
what is on its own links.

```verify
ping PC1 2001:db8:68:2::10 => failure
ping PC1 2001:db8:68:1::1 => success
```

## Part 2 — Fault 1: a router that does not forward

Look at the routers from the middle outwards. R2 is the next router after R1, and the first
question for a router that is not forwarding is whether it has been told to.

```verify
R2# show ipv6 route
  expect: IPv6 forwarding is off
R2# show ipv6 interface g0/0
  expect: Not routing IPv6
R1# show ipv6 route
  reject: IPv6 forwarding is off
```

Without `ipv6 unicast-routing` a router is a host as far as IPv6 is concerned: it answers for
its own addresses and forwards nothing, and the symptoms stop at it. Turn it on.

```console R2
configure terminal
ipv6 unicast-routing
end
```

```verify
R2# show ipv6 route
  reject: IPv6 forwarding is off
ping PC1 2001:db8:68:2::10 => failure
```

## Part 3 — Fault 2: a next hop that does not exist

Forwarding is on everywhere and the ping still fails, so look at the route R1 uses for PC2's
network, and at whom it names as the next hop.

```verify
R1# show ipv6 route static
  expect: 2001:db8:68:2::/64
  expect: via 2001:db8:68:12::3
ping PC1 2001:db8:68:12::2 => success
```

The next hop must be an address on a shared link, owned by a router that exists. The link
between R1 and R2 has R1 at ::1 and R2 at ::2: nobody has ::3, so R1 can find no neighbour to
hand the packet to. Replace the route.

```console R1
configure terminal
no ipv6 route 2001:db8:68:2::/64 2001:db8:68:12::3
ipv6 route 2001:db8:68:2::/64 2001:db8:68:12::2
end
```

```verify
R1# show ipv6 route static
  expect: via 2001:db8:68:12::2
  reject: 2001:db8:68:12::3
ping PC1 2001:db8:68:2::10 => failure
```

## Part 4 — Fault 3: a host with the wrong gateway

The packet now crosses both routers and arrives. The reply is the problem: PC2 sends it to its
gateway, and 2001:db8:68:2::2 is not a router. A host reaches what is on its own link by
neighbour discovery, and what is not through the gateway, which it must be able to find the
same way.

```verify
ping PC2 2001:db8:68:2::1 => success
PC2> ipv6config
  expect: 2001:db8:68:2::2
```

PC2 reaches R2 (on its link) and not PC1 (beyond it). Correct the gateway.

```console PC2
ipv6config 2001:db8:68:2::10/64 2001:db8:68:2::1
```

```verify
ping PC1 2001:db8:68:2::10 => success
ping PC2 2001:db8:68:1::10 => success
PC2> ipv6config
  expect: 2001:db8:68:2::1
```

## Reflection

1. Why does the symptom "ping fails" point at the first router that is wrong, and not at the
   last one?
2. What does a router with no `ipv6 unicast-routing` still do for IPv6, and what does it not?
3. A static route to a next hop that nobody has: how is it different from a route to a router that
   exists but is down?
4. Which of the three faults would you have found by comparing the two routers' running
   configurations, and which only by following a packet?
