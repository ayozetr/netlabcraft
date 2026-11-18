# Lab 60 — BGP for IPv6: address families, next hops and policy

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 60 minutes

## Objectives

- Carry IPv6 prefixes in BGP: open an `address-family ipv6`, activate neighbours in it and
  advertise prefixes with `network`.
- Run the same family over two kinds of session: one over IPv6 (the neighbour is an IPv6 address)
  and one over IPv4 (the neighbour is an IPv4 address that also carries IPv6 prefixes).
- See why an internal path is not usable until its next hop resolves, and fix it with
  `next-hop-self`.
- Read the IPv6 table (`show bgp ipv6 unicast`), the routes BGP installs (`show ipv6 route bgp`,
  distance 20 for external paths and 200 for internal ones) and a neighbour's view.
- Filter what a neighbour is sent with an `ipv6 prefix-list`.

## Topology

```text
 R1 ───────────── R2 ───────────── R3
 AS 65001  2001:db8:12::/64  AS 65002  10.60.23.0/30 and 2001:db8:23::/64  AS 65002
 Lo1 2001:db8:1::/64                                                 Lo1 2001:db8:3::/64
```

| Device | Model | Interface | Address | Runs |
|---|---|---|---|---|
| R1 | Router-2 | G0/1 | 2001:db8:12::1/64 | BGP AS 65001 |
| R1 | | Loopback1 | 2001:db8:1::1/64 | |
| R2 | Router-2 | G0/0 | 2001:db8:12::2/64 | BGP AS 65002 |
| R2 | | G0/1 | 10.60.23.1/30, 2001:db8:23::2/64 | |
| R3 | Router-2 | G0/0 | 10.60.23.2/30, 2001:db8:23::3/64 | BGP AS 65002 |
| R3 | | Loopback1 | 2001:db8:3::1/64 | |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/0"}
```

```console R1
enable
configure terminal
ipv6 unicast-routing
interface g0/1
ipv6 address 2001:db8:12::1/64
no shutdown
interface loopback 1
ipv6 address 2001:db8:1::1/64
end
```

```console R2
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ipv6 address 2001:db8:12::2/64
no shutdown
interface g0/1
ip address 10.60.23.1 255.255.255.252
ipv6 address 2001:db8:23::2/64
no shutdown
end
```

```console R3
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ip address 10.60.23.2 255.255.255.252
ipv6 address 2001:db8:23::3/64
no shutdown
interface loopback 1
ipv6 address 2001:db8:3::1/64
end
```

## Part 1 — A family of its own

A BGP router carries IPv4 prefixes unless told otherwise. IPv6 prefixes travel in a family of their
own, `address-family ipv6`, and a neighbour takes part in it only when it is activated there. Both
ends have to do it: the OPEN message of each announces the family, and prefixes flow only for a
family both announced.

R1 and R2 are neighbours over IPv6: the neighbour is named by its IPv6 address, the session runs in
IPv6 packets, and the next hop of every prefix is an IPv6 address. R2 and R3 are neighbours over
IPv4: the session is the one BGP always had, and it also carries IPv6 prefixes once both ends have
activated the neighbour in `address-family ipv6`.

```console R1
configure terminal
router bgp 65001
bgp router-id 1.1.1.1
neighbor 2001:db8:12::2 remote-as 65002
address-family ipv6
neighbor 2001:db8:12::2 activate
network 2001:db8:1::/64
end
```

```console R2
configure terminal
router bgp 65002
bgp router-id 2.2.2.2
neighbor 2001:db8:12::1 remote-as 65001
neighbor 10.60.23.2 remote-as 65002
address-family ipv6
neighbor 2001:db8:12::1 activate
neighbor 10.60.23.2 activate
end
```

```console R3
configure terminal
router bgp 65002
bgp router-id 3.3.3.3
neighbor 10.60.23.1 remote-as 65002
address-family ipv6
neighbor 10.60.23.1 activate
network 2001:db8:3::/64
end
```

```lab
sim.advance {"seconds": 8}
```

The sessions need simulated time to reach Established, which is why the clock was advanced.

```verify
R1# show bgp ipv6 unicast summary
  expect: Address family: IPv6 unicast
  expect: 2001:db8:12::2
  expect: IPv6
R2# show bgp ipv6 unicast summary
  expect: 10.60.23.2
  expect: IPv4
  expect: 2001:db8:12::1
R2# show bgp ipv6 unicast neighbors 2001:db8:12::1
  expect: Session over IPv6, state Established
R2# show bgp ipv6 unicast neighbors 10.60.23.2
  expect: Session over IPv4, state Established
  expect: Address family IPv6 unicast: negotiated
```

## Part 2 — A next hop that cannot be reached

R2 learned 2001:db8:1::/64 from R1 and passed it on to R3 as it was, with R1's address as the
next hop. R3 has no route to that address, so the path is in its table but it is not usable and
not installed.

```verify
R2# show ipv6 route bgp
  expect: 2001:db8:1::/64
  expect: [20]
  expect: 2001:db8:3::/64
  expect: [200]
R3# show bgp ipv6 unicast 2001:db8:1::/64
  expect: not usable
R3# show ipv6 route bgp
  reject: 2001:db8:1::/64
```

The same rule that applies to IPv4 applies here: an internal neighbour keeps the next hop a path
came with, so a router at the edge of the AS makes itself the next hop with `next-hop-self`, now
in the IPv6 family.

```console R2
configure terminal
router bgp 65002
address-family ipv6
neighbor 10.60.23.2 next-hop-self
end
```

```lab
sim.advance {"seconds": 4}
```

```verify
R3# show ipv6 route bgp
  expect: 2001:db8:1::/64
  expect: via 2001:db8:23::2
  expect: [200]
R3# show bgp ipv6 unicast 2001:db8:1::/64
  expect: internal, best
R1# show ipv6 route bgp
  expect: 2001:db8:3::/64
  expect: [20]
```

## Part 3 — Next hops with a link-local address

Between external neighbours on one link the next hop has two parts: the global address, which is
what the routing table resolves, and the link-local address of the same interface. The entry shows
both.

```verify
R1# show bgp ipv6 unicast 2001:db8:3::/64
  expect: Next hop 2001:db8:12::2 (link-local fe80::
  expect: external, best
```

## Part 4 — Redistribute what is connected and reach the far end

R3 does not know the link between R1 and R2, so its answers to R1 would have nowhere to go. R2
advertises its connected networks with `redistribute connected` in the family; their origin is
`incomplete`.

```console R2
configure terminal
router bgp 65002
address-family ipv6
redistribute connected
end
```

```lab
sim.advance {"seconds": 4}
```

```verify
R3# show ipv6 route bgp
  expect: 2001:db8:12::/64
R1# ping 2001:db8:3::1
  expect: !
R3# ping 2001:db8:1::1
  expect: !
```

## Part 5 — Filter with an IPv6 prefix list

R1 does not need the network between R2 and R3. R2 stops sending it with an `ipv6 prefix-list` on
the neighbour, applied outward in the family.

```console R2
configure terminal
ipv6 prefix-list NO23 seq 5 deny 2001:db8:23::/64
ipv6 prefix-list NO23 seq 10 permit ::/0 le 128
router bgp 65002
address-family ipv6
neighbor 2001:db8:12::1 prefix-list NO23 out
end
```

```lab
sim.advance {"seconds": 4}
```

```verify
R1# show bgp ipv6 unicast
  reject: 2001:db8:23::/64
  expect: 2001:db8:3::/64
R2# show ipv6 prefix-list NO23
  expect: seq 5 deny 2001:db8:23::/64
R2# show running-config
  expect: neighbor 2001:db8:12::1 prefix-list NO23 out
  expect: ipv6 prefix-list NO23 seq 10 permit ::/0 le 128
R1# ping 2001:db8:3::1
  expect: !
```

## What you practised

- `address-family ipv6`, `neighbor … activate` and what happens when only one end activates.
- Neighbours named by an IPv6 address and by an IPv4 address, in the same family.
- `network`, `redistribute connected` and `next-hop-self` in the family.
- Reading `show bgp ipv6 unicast`, `show ipv6 route bgp` and a neighbour's IPv6 view.
- `ipv6 prefix-list` on a neighbour.
