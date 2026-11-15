# Lab 33 — IPv6 routing: static routes, OSPFv3 and a default route

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Route IPv6 between three routers with static routes, and read `show ipv6 route`.
- Replace the static routes with OSPFv3, and see that the neighbours talk over link-local
  addresses.
- Hand a default route to the whole network from the router at the edge.
- Check each stage with `ping` and `tracert` to IPv6 addresses.

## Topology

```text
 PC1 ── G0/1  R1  G0/0 ──── G0/0  R2  G0/1 ──── G0/0  R3  G0/1 ── PC3
                                                      G0/2
                                                       │
                                                  G0/0 ISP  G0/1 ── WEB
```

| Device | Model | Interface | IPv6 address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 2001:db8:33:12::1/64 |
| R1 | | G0/1 | 2001:db8:33:1::1/64 |
| R2 | Router-2 | G0/0 | 2001:db8:33:12::2/64 |
| R2 | | G0/1 | 2001:db8:33:23::2/64 |
| R3 | Router-3 | G0/0 | 2001:db8:33:23::3/64 |
| R3 | | G0/1 | 2001:db8:33:3::1/64 |
| R3 | | G0/2 | 2001:db8:33:99::1/64 |
| ISP | Router-2 | G0/0 | 2001:db8:33:99::2/64 |
| ISP | | G0/1 | 2001:db8:fe::1/64 |
| PC1 | PC | Fa0 | 2001:db8:33:1::10/64, gateway 2001:db8:33:1::1 |
| PC3 | PC | Fa0 | 2001:db8:33:3::10/64, gateway 2001:db8:33:3::1 |
| WEB | PC | Fa0 | 2001:db8:fe::10/64, gateway 2001:db8:fe::1 |

R1, R2 and R3 are the company's routers. The ISP and WEB are added in Part 3 and stand for the
Internet. Everything in the company lives inside `2001:db8:33::/48`.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-3", "name": "R3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/0", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/1"}
link.add {"a": "PC3", "b": "R3", "b_port": "g0/1"}
```

```console PC1
ipv6config 2001:db8:33:1::10/64 2001:db8:33:1::1
```

```console PC3
ipv6config 2001:db8:33:3::10/64 2001:db8:33:3::1
```

## Part 1 — Static routes

A router only forwards IPv6 after `ipv6 unicast-routing`. Each router knows its own links;
the networks further away need a route that names the next hop on a shared link.

```console R1
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ipv6 address 2001:db8:33:12::1/64
no shutdown
interface g0/1
ipv6 address 2001:db8:33:1::1/64
no shutdown
exit
ipv6 route 2001:db8:33:23::/64 2001:db8:33:12::2
ipv6 route 2001:db8:33:3::/64 2001:db8:33:12::2
end
```

```console R2
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ipv6 address 2001:db8:33:12::2/64
no shutdown
interface g0/1
ipv6 address 2001:db8:33:23::2/64
no shutdown
exit
ipv6 route 2001:db8:33:1::/64 2001:db8:33:12::1
ipv6 route 2001:db8:33:3::/64 2001:db8:33:23::3
end
```

```console R3
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ipv6 address 2001:db8:33:23::3/64
no shutdown
interface g0/1
ipv6 address 2001:db8:33:3::1/64
no shutdown
exit
ipv6 route 2001:db8:33:1::/64 2001:db8:33:23::2
ipv6 route 2001:db8:33:12::/64 2001:db8:33:23::2
end
```

```verify
R1# show ipv6 interface brief
  expect: 2001:db8:33:12::1
R1# show ipv6 route static
  expect: 2001:db8:33:3::/64
  expect: via 2001:db8:33:12::2
R1# show ipv6 route
  expect: C  2001:db8:33:1::/64
  expect: S  2001:db8:33:23::/64
ping PC1 2001:db8:33:3::10 => success
ping PC3 2001:db8:33:1::10 => success
PC1> tracert 2001:db8:33:3::10
  expect: 2001:db8:33:1::1
  expect: 2001:db8:33:12::2
  expect: 2001:db8:33:23::3
```

Each line of the trace is the address of the interface a router received the probe on. Six
static routes keep three routers talking, and every new network would add routes on every
router, so Part 2 lets the routers learn them.

## Part 2 — OSPFv3

OSPFv3 carries IPv6 prefixes. It is enabled on each interface with `ipv6 ospf PROCESS area
AREA` rather than with `network` statements, and it needs a 32-bit router ID, written like an
IPv4 address, because these routers have no IPv4 address to take one from. First the static
routes come out, so the routing table shows what OSPFv3 teaches:

```console R1
configure terminal
no ipv6 route 2001:db8:33:23::/64
no ipv6 route 2001:db8:33:3::/64
ipv6 router ospf 1
router-id 1.1.1.1
passive-interface g0/1
exit
interface g0/0
ipv6 ospf 1 area 0
interface g0/1
ipv6 ospf 1 area 0
end
```

```console R2
configure terminal
no ipv6 route 2001:db8:33:1::/64
no ipv6 route 2001:db8:33:3::/64
ipv6 router ospf 1
router-id 2.2.2.2
exit
interface g0/0
ipv6 ospf 1 area 0
interface g0/1
ipv6 ospf 1 area 0
end
```

```console R3
configure terminal
no ipv6 route 2001:db8:33:1::/64
no ipv6 route 2001:db8:33:12::/64
ipv6 router ospf 1
router-id 3.3.3.3
passive-interface g0/1
exit
interface g0/0
ipv6 ospf 1 area 0
interface g0/1
ipv6 ospf 1 area 0
end
```

```lab
sim.advance {"seconds": 40}
```

```verify
R2# show ipv6 ospf neighbor
  expect: 1.1.1.1
  expect: 3.3.3.3
  expect: FULL
  expect: fe80::
R1# show ipv6 ospf
  expect: Router id 1.1.1.1
  expect: area 0, cost 1, passive
R1# show ipv6 route ospf
  expect: 2001:db8:33:3::/64
  expect: 2001:db8:33:23::/64
  expect: via fe80::
R1# show ipv6 route static
  expect: (no such routes)
R3# show ipv6 route ospf
  expect: 2001:db8:33:1::/64
ping PC1 2001:db8:33:3::10 => success
ping PC3 2001:db8:33:1::10 => success
PC1> tracert 2001:db8:33:3::10
  expect: 2001:db8:33:12::2
  expect: 2001:db8:33:23::3
```

The routes are the same and the hand-written ones are gone. The next hops are link-local
addresses (`fe80::…`): OSPFv3 routers talk to the neighbour on the link and never need a
global address for it. The LAN interfaces are passive, so they are advertised without
sending hellos towards the PCs.

## Part 3 — A default route from the edge

R3 gets an uplink to an ISP. The ISP stands for the Internet; it knows the way back to the
company's `/48`, and WEB is a server on its network.

```lab
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "PC", "name": "WEB"}
link.add {"a": "R3", "a_port": "g0/2", "b": "ISP", "b_port": "g0/0"}
link.add {"a": "WEB", "b": "ISP", "b_port": "g0/1"}
```

```console WEB
ipv6config 2001:db8:fe::10/64 2001:db8:fe::1
```

```console ISP
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ipv6 address 2001:db8:33:99::2/64
no shutdown
interface g0/1
ipv6 address 2001:db8:fe::1/64
no shutdown
exit
ipv6 route 2001:db8:33::/48 2001:db8:33:99::1
end
```

R3 reaches the Internet with a static default route (`::/0`), and tells the other routers
about it with `default-information originate`. The uplink itself is not in OSPFv3: the ISP is
not part of the company's routing.

```console R3
configure terminal
interface g0/2
ipv6 address 2001:db8:33:99::1/64
no shutdown
exit
ipv6 route ::/0 2001:db8:33:99::2
ipv6 router ospf 1
default-information originate
end
```

```lab
sim.advance {"seconds": 40}
```

```verify
R3# show ipv6 route static
  expect: ::/0
  expect: via 2001:db8:33:99::2
R1# show ipv6 route
  expect: OE2 ::/0
R2# show ipv6 route
  expect: OE2 ::/0
ping PC1 2001:db8:fe::10 => success
ping PC3 2001:db8:fe::10 => success
ping WEB 2001:db8:33:1::10 => success
PC1> tracert 2001:db8:fe::10
  expect: 2001:db8:33:12::2
  expect: 2001:db8:33:23::3
  expect: 2001:db8:33:99::2
```

`OE2` is an external OSPFv3 route: it came from outside OSPFv3 (R3's static default). R1 and
R2 never had a route to the ISP's network, yet they reach it, because the default sends
everything they do not know towards R3. The return traffic works because the ISP has the
`/48` pointing back at R3, and R3 learned the LANs through OSPFv3.

## Reflection

1. Which kind of address did the OSPFv3 routes use as next hop, and why does that work with no
   global address on the link?
2. Why does a router that has no IPv4 address need `router-id` for OSPFv3?
3. What would you have to add by hand, on how many routers, to give the company a new LAN
   with static routes? And with OSPFv3?
4. Why is the link to the ISP not part of OSPFv3, and what carries its route to the others?
