# Lab 87 — Multicast routing with PIM: trees built for the receivers

**Course:** CCNP ENARSI follow-up (multicast) · **Time:** 60 minutes

## Objectives

- Turn on multicast routing and PIM sparse mode, name a rendezvous point, and see the routers find each other
  (`show ip pim neighbor`, `show ip pim interface`).
- Join a group from a PC and read the shared tree it builds towards the rendezvous point
  (`show ip mroute`, the `(*, G)` entry).
- Send to the group from the other side of the network, and read the source tree and its reverse path check
  (the `(S, G)` entry, `show ip mroute count`).
- Break the tree on purpose: no rendezvous point, no PIM on a link. Join a group from a router interface.

## Topology

```text
 PC1 ── R1 ── R2 ── PC2
   source   │     receiver
            └ R1's Loopback0 (1.1.1.1) is the rendezvous point
```

| Device | Model | Interface | Address |
|---|---|---|---|
| PC1 | PC | Fa0 | 10.87.1.10/24, gateway 10.87.1.1 (the sender) |
| R1 | Router-2 | G0/0 | 10.87.1.1/24 |
| R1 | | G0/1 | 10.87.12.1/24 |
| R1 | | Loopback0 | 1.1.1.1/32 |
| R2 | Router-2 | G0/1 | 10.87.12.2/24 |
| R2 | | G0/0 | 10.87.2.1/24 |
| PC2 | PC | Fa0 | 10.87.2.10/24, gateway 10.87.2.1 (the receiver) |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "R2", "a_port": "g0/0", "b": "PC2"}
```

```console PC1
ipconfig 10.87.1.10 255.255.255.0 10.87.1.1
```

```console PC2
ipconfig 10.87.2.10 255.255.255.0 10.87.2.1
```

Static routes carry the unicast traffic. PIM does not route anything itself: it asks the unicast table which way
the source and the rendezvous point are (the reverse path), so the table matters.

```console R1
enable
configure terminal
hostname R1
interface g0/0
ip address 10.87.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.87.12.1 255.255.255.0
no shutdown
interface loopback0
ip address 1.1.1.1 255.255.255.255
exit
ip route 10.87.2.0 255.255.255.0 10.87.12.2
end
```

```console R2
enable
configure terminal
hostname R2
interface g0/0
ip address 10.87.2.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.87.12.2 255.255.255.0
no shutdown
exit
ip route 10.87.1.0 255.255.255.0 10.87.12.1
ip route 1.1.1.1 255.255.255.255 10.87.12.1
end
```

The first packet of any conversation waits for ARP and is lost, so the two PCs ping each other once to let
the routers learn the addresses on the way. Without this the first multicast ping below would lose its answer.

```verify
PC1> ping -n 3 10.87.2.10
  expect: reply from 10.87.2.10
PC2> ping -n 3 10.87.1.10
  expect: reply from 10.87.1.10
```

## Part 1 — Multicast routing and PIM

`ip multicast-routing` turns the router into a multicast router. PIM runs per interface, and every router needs to
know which router is the rendezvous point (RP) of the groups.

```console R1
configure terminal
ip multicast-routing
ip pim rp-address 1.1.1.1
interface g0/0
ip pim sparse-mode
interface g0/1
ip pim sparse-mode
interface loopback0
ip pim sparse-mode
end
```

```console R2
configure terminal
ip multicast-routing
ip pim rp-address 1.1.1.1
interface g0/0
ip pim sparse-mode
interface g0/1
ip pim sparse-mode
end
```

```verify
R1# show ip pim neighbor
  expect: 10.87.12.2
  expect: designated router
R2# show ip pim neighbor
  expect: 10.87.12.1
R1# show ip pim interface
  expect: Sparse
  expect: Loopback0
R2# show ip pim rp mapping
  expect: 1.1.1.1
```

R2's address on the shared link is higher, so it is the designated router there. The designated router is the one
that forwards a group's packets onto a link that has members.

## Part 2 — A receiver joins: the shared tree

```console PC2
igmp join 239.1.1.1
```

R2 asks for members on its LAN and PC2 answers, so R2 knows the group has a member on G0/0. It joins towards the
rendezvous point along the path the unicast table gives, and R1 (the rendezvous point) adds the link to R2.

```verify
R2# show ip igmp groups
  expect: 239.1.1.1
  expect: 10.87.2.10
R2# show ip mroute
  expect: (*, 239.1.1.1)
  expect: RP 1.1.1.1
  expect: Incoming interface: GigabitEthernet0/1
  expect: GigabitEthernet0/0, Forward/Sparse
R1# show ip mroute
  expect: (*, 239.1.1.1)
  expect: Incoming interface: Null
  expect: GigabitEthernet0/1, Forward/Sparse
```

## Part 3 — A source sends: the source tree

PC1 pings the group. Its first router registers the source with the rendezvous point, and every router with a
receiver below it joins the shortest path to the source.

```verify
PC1> ping -n 1 239.1.1.1
  expect: reply from 10.87.2.10
R1# show ip mroute
  expect: (10.87.1.10, 239.1.1.1)
  expect: the source is on this link
R2# show ip mroute 239.1.1.1
  expect: (10.87.1.10, 239.1.1.1)
  expect: RPF neighbour 10.87.12.1
R2# show ip mroute count
  expect: Source 10.87.1.10
  expect: 1 packets forwarded
R2# show ip mroute summary
  expect: 1 (*,G) entry and 1 (S,G) entry
```

Every packet is checked against the reverse path: it must arrive on the interface the unicast table would use to
reach the source. A packet that arrives anywhere else is dropped, which is what stops multicast from looping.

## Part 4 — Break it

Without a rendezvous point, a sparse-mode source cannot register and the receiver cannot join.

```console R1
configure terminal
no ip pim rp-address 1.1.1.1
end
```

```console R2
configure terminal
no ip pim rp-address 1.1.1.1
end
```

```verify
PC1> ping -n 1 239.1.1.1
  expect: no answer
R2# show ip pim rp mapping
  expect: No rendezvous point
```

Put it back, then take PIM off the link between the routers: the tree has no way across.

```console R1
configure terminal
ip pim rp-address 1.1.1.1
end
```

```console R2
configure terminal
ip pim rp-address 1.1.1.1
interface g0/1
no ip pim sparse-mode
end
```

```verify
PC1> ping -n 1 239.1.1.1
  expect: no answer
R1# show ip pim neighbor
  expect: No PIM neighbours
```

## Part 5 — A router that is a member

```console R2
configure terminal
interface g0/1
ip pim sparse-mode
exit
interface g0/0
ip igmp join-group 239.9.9.9
end
```

```verify
R2# show ip igmp groups
  expect: 239.9.9.9
  expect: this router
PC1> ping -n 1 239.9.9.9
  expect: reply from 10.87.12.2
```

R2 asked for the group on G0/0 as a host would, so the tree reaches it and it answers the ping itself.

## Reflection

1. Why does PIM not exchange routes, and what does that make the unicast routing table for multicast?
2. What is checked by the reverse path check, and what would happen to multicast on a ring without it?
3. Which router becomes the designated router of a link, and what does it do?
4. In dense mode (`ip pim dense-mode`) there is no rendezvous point. How does a router learn who wants the group?
