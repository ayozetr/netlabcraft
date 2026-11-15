# Lab 36 — BGP between two autonomous systems

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 75 minutes

## Objectives

- Start a BGP process with `router bgp`, name neighbours with `neighbor … remote-as`, and
  advertise a network with `network … mask …`.
- Tell an external (eBGP) session from an internal (iBGP) one, and read `show ip bgp summary`,
  `show ip bgp neighbors` and `show ip bgp`.
- See why a path whose next hop cannot be reached is not used, and repair it with
  `next-hop-self`.
- Explain why a `network` statement advertises nothing until that exact prefix is in the routing
  table.
- Follow BGP's routes into the routing table (`B`, administrative distance 20 and 200), break
  the link, and restart a session with `clear ip bgp`.

## Topology

```text
 AS 65001                      AS 65002
                    .------------------------.
 PC1 ── R1 ─────── R2 ─────────────────── R3 ── PC3
 192.168.1.0/24   10.36.12.0/30   10.36.23.0/30   192.168.3.0/24
```

| Device | Model | Interface | Address | BGP router ID |
|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 | 1.1.1.1 |
| R1 | | G0/1 | 10.36.12.1/30 | |
| R2 | Router-3 | G0/0 | 10.36.12.2/30 | 2.2.2.2 |
| R2 | | G0/1 | 10.36.23.1/30 | |
| R3 | Router-2 | G0/0 | 192.168.3.1/24 | 3.3.3.3 |
| R3 | | G0/1 | 10.36.23.2/30 | |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw 192.168.1.1 | |
| PC3 | PC | Fa0 | 192.168.3.10/24, gw 192.168.3.1 | |

R1 is alone in autonomous system 65001. R2 and R3 share autonomous system 65002. There is no
other routing protocol: everything R1 and R3 know about each other's LAN, BGP tells them.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-3", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC3", "b": "R3", "b_port": "g0/0"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC3
ipconfig 192.168.3.10 255.255.255.0 192.168.3.1
```

## Part 1 — Addresses

Give every interface its address. The links between routers are `/30` networks.

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.36.12.1 255.255.255.252
no shutdown
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.36.12.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.36.23.1 255.255.255.252
no shutdown
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 192.168.3.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.36.23.2 255.255.255.252
no shutdown
end
```

```verify
ping PC1 192.168.1.1 => success
ping PC3 192.168.3.1 => success
R1# ping 10.36.12.2
  expect: !!!
R3# ping 10.36.23.1
  expect: !!!
```

Neighbours across a link can talk, but R1 cannot reach R3's LAN: nothing tells it where it is.

```verify
ping PC1 192.168.3.10 => failure
```

## Part 2 — An external session between R1 and R2

`router bgp N` starts the process in autonomous system N. A neighbour in a different AS is an
*external* neighbour: the session is eBGP. By default an external neighbour must be on a
directly connected network. A `network` statement names a prefix to advertise.

```console R1
configure terminal
router bgp 65001
bgp router-id 1.1.1.1
bgp log-neighbor-changes
neighbor 10.36.12.2 remote-as 65002
network 192.168.1.0 mask 255.255.255.0
end
```

```console R2
configure terminal
router bgp 65002
bgp router-id 2.2.2.2
neighbor 10.36.12.1 remote-as 65001
end
```

A session does not come up at once. It goes through Connect (the TCP connection), OpenSent and
OpenConfirm (the two routers exchange OPEN and KEEPALIVE messages) before it is Established and
routes are sent. In NetLabCraft that takes about three seconds of simulated time.

```lab
sim.advance {"seconds": 10}
```

```verify
R1# show ip bgp summary
  expect: BGP router id 1.1.1.1, local AS 65001
  expect: 10.36.12.2
  expect: 65002
R1# show ip bgp neighbors 10.36.12.2
  expect: remote AS 65002 (external link)
  expect: State: Established
  expect: Neighbour router id: 2.2.2.2
  expect: Messages sent
R2# show ip bgp
  expect: 192.168.1.0/24
  expect: 65001 i
R2# show ip route bgp
  expect: B    192.168.1.0/24
  expect: via 10.36.12.1, GigabitEthernet0/0 (BGP) [20/0]
R1# show logging
  expect: %BGP-5-ADJCHANGE: neighbor 10.36.12.2 Up
```

`B` is BGP's code in the routing table. `[20/0]` is the administrative distance, 20 for a path
learned from an external neighbour, and the metric (the MED, 0 here). In `show ip bgp`, `*`
marks a usable path, `>` the best one, and the path column lists the autonomous systems the
path crossed, then its origin (`i`: from a `network` statement).

## Part 3 — An internal session between R2 and R3

R2 and R3 are in the same AS, so their session is iBGP. An internal neighbour may be anywhere
the routing table can reach, and BGP does not change the next hop of a path it passes between
internal neighbours. Start the session, and advertise R3's LAN:

```console R2
configure terminal
router bgp 65002
neighbor 10.36.23.2 remote-as 65002
end
```

```console R3
configure terminal
router bgp 65002
bgp router-id 3.3.3.3
neighbor 10.36.23.1 remote-as 65002
network 192.168.3.0 mask 255.255.255.0
end
```

```lab
sim.advance {"seconds": 10}
```

```verify
R2# show ip bgp neighbors 10.36.23.2
  expect: (internal link)
  expect: State: Established
R1# show ip route bgp
  expect: B    192.168.3.0/24
  expect: via 10.36.12.2, GigabitEthernet0/1 (BGP) [20/0]
R2# show ip route bgp
  expect: B    192.168.3.0/24
  expect: [200/0]
```

R1 learned R3's LAN from R2, and R2 learned it from R3 with administrative distance 200: an
internal path loses to nearly every other source. Now look at the way back. R2 received R1's
LAN from R1, and passed it to R3 unchanged, so its next hop is still R1's address,
`10.36.12.1`, which R3 has no route to:

```verify
R3# show ip bgp 192.168.1.0
  expect: BGP entry for 192.168.1.0/24: 1 path, none usable (no next hop resolves)
  expect: next hop 10.36.12.1
  expect: internal, next hop unreachable
R3# show ip route bgp
  reject: 192.168.1.0/24
ping PC1 192.168.3.10 => failure
```

The ping fails on the way back: PC3's reply reaches R3, and R3 has no usable route to
`192.168.1.0/24`. Make R2 name itself as the next hop of what it sends to R3:

```console R2
configure terminal
router bgp 65002
neighbor 10.36.23.2 next-hop-self
end
```

```verify
R3# show ip route bgp
  expect: B    192.168.1.0/24
  expect: via 10.36.23.1, GigabitEthernet0/1 (BGP) [200/0]
R2# show ip bgp neighbors 10.36.23.2
  expect: Options: next-hop-self
ping PC1 192.168.3.10 => success
ping PC3 192.168.1.10 => success
```

## Part 4 — A network statement needs the prefix in the routing table

`network` does not create anything: BGP advertises the prefix only while a route for exactly
that prefix is in the routing table. Name a prefix R1 does not have:

```console R1
configure terminal
router bgp 65001
network 172.16.0.0 mask 255.255.0.0
end
```

```verify
R2# show ip bgp
  reject: 172.16.0.0/16
R3# show ip route bgp
  reject: 172.16.0.0/16
```

Nothing is advertised. Give R1 a connected network with that prefix, a loopback:

```console R1
configure terminal
interface loopback 10
ip address 172.16.0.1 255.255.0.0
end
```

```verify
R2# show ip bgp
  expect: 172.16.0.0/16
R3# show ip route bgp
  expect: 172.16.0.0/16
```

Change the loopback's mask to `255.255.255.0` and the prefix in the table is `172.16.0.0/24`,
not the `/16` the statement names: the advertisement is withdrawn again.

```console R1
configure terminal
interface loopback 10
ip address 172.16.0.1 255.255.255.0
end
```

```verify
R2# show ip bgp
  reject: 172.16.0.0/16
```

Put the mask back, so the next parts have the prefix:

```console R1
configure terminal
interface loopback 10
ip address 172.16.0.1 255.255.0.0
end
```

## Part 5 — Reading one prefix

`show ip bgp A.B.C.D` lists every path to a destination, which one is best, and what BGP knows
about it. R2 learned the prefix from an external neighbour, R3 from an internal one; the AS
path is the same, because iBGP does not add the AS number:

```verify
R2# show ip bgp 172.16.0.0
  expect: BGP entry for 172.16.0.0/16: 1 path, the best is path 1
  expect: AS path 65001
  expect: origin IGP
  expect: external, usable
R3# show ip bgp 172.16.0.0
  expect: AS path 65001
  expect: internal, usable
R1# show ip bgp 172.16.0.0
  expect: AS path local
  expect: originated here
```

When several paths to a prefix exist, BGP picks one by the highest weight, then the highest local
preference, a path this router originated, the shortest AS path, the lowest origin, the lowest
MED, an external path over an internal one, and finally the lowest router ID.

## Part 6 — Breaking the link

Shut the link between R1 and R2. A session cannot outlive the path it runs over, and the routes
it carried go with it.

```console R1
configure terminal
interface g0/1
shutdown
end
```

```verify
R2# show ip bgp summary
  expect: Idle
R2# show ip route bgp
  reject: 192.168.1.0/24
R3# show ip route bgp
  reject: 192.168.1.0/24
R1# show logging
  expect: neighbor 10.36.12.2 Down
ping PC3 192.168.1.10 => failure
```

Bring it back. The session starts from the beginning and needs its three seconds again:

```console R1
configure terminal
interface g0/1
no shutdown
end
```

```lab
sim.advance {"seconds": 10}
```

```verify
R3# show ip route bgp
  expect: 192.168.1.0/24
ping PC3 192.168.1.10 => success
```

`clear ip bgp *` resets the sessions of a router on purpose: they go down and start again.

```console R1
clear ip bgp *
```

```verify
R1# show ip bgp neighbors 10.36.12.2
  expect: State: Connect
R3# show ip route bgp
  reject: 192.168.1.0/24
```

```lab
sim.advance {"seconds": 10}
```

```verify
R1# show ip bgp neighbors 10.36.12.2
  expect: State: Established
R3# show ip route bgp
  expect: 192.168.1.0/24
R1# show running-config
  expect: router bgp 65001
  expect: neighbor 10.36.12.2 remote-as 65002
  expect: network 192.168.1.0 mask 255.255.255.0
```

## Reflection

1. Why is the session between R1 and R2 refused if R1 names AS 65003 for R2, and what does
   `show ip bgp neighbors` say about it?
2. R3 had R1's LAN in its BGP table in Part 3 and still did not use it. What made the path
   unusable, and why did `next-hop-self` on R2 fix it?
3. Why does the same prefix carry administrative distance 20 on R2 and 200 on R3?
4. What would you have to change for R1 and R2 to peer over loopback addresses instead of the
   link between them?
