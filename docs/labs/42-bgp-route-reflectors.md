# Lab 42 — BGP route reflectors

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 60 minutes

## Objectives

- Explain why a plain iBGP router does not pass on what another iBGP router told it, and what that
  costs: a full mesh of sessions.
- Make a router a route reflector and name its clients (`neighbor … route-reflector-client`).
- Apply the reflection rules: a path from a client goes to every internal neighbour, a path from a
  non-client only to the clients.
- Read the two attributes a reflector adds, ORIGINATOR_ID and CLUSTER_LIST, in `show ip bgp`, and
  name a cluster with `bgp cluster-id`.
- See how the attributes keep a path from coming back to where it started.

## Topology

```text
                  R1 (client)
                  172.16.1.0/24
                   │ 10.42.12.0/30
 R3 (client) ───── R2 (reflector) ───── R4
 172.16.3.0/24   10.42.23.0/30   10.42.24.0/30   172.16.4.0/24
```

| Device | Model | Interface | Address | BGP router ID |
|---|---|---|---|---|
| R1 | Router-2 | G0/1 | 10.42.12.1/30 | 1.1.1.1 |
| R1 | | Loopback1 | 172.16.1.1/24 | |
| R2 | Router-3 | G0/0 | 10.42.12.2/30 | 2.2.2.2 |
| R2 | | G0/1 | 10.42.23.1/30 | |
| R2 | | G0/2 | 10.42.24.1/30 | |
| R3 | Router-2 | G0/1 | 10.42.23.2/30 | 3.3.3.3 |
| R3 | | Loopback3 | 172.16.3.1/24 | |
| R4 | Router-2 | G0/1 | 10.42.24.2/30 | 4.4.4.4 |
| R4 | | Loopback4 | 172.16.4.1/24 | |

All four routers are in AS 65001 and each leaf has an iBGP session with R2 only. OSPF carries the
addresses of the links so every router can reach every BGP next hop; BGP carries the four
`172.16.x.0/24` prefixes.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-3", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
device.add {"model": "Router-2", "name": "R4"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R3", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "R4", "a_port": "g0/1", "b": "R2", "b_port": "g0/2"}
```

## Part 1 — Without a reflector

```console R1
enable
configure terminal
interface g0/1
ip address 10.42.12.1 255.255.255.252
no shutdown
interface loopback 1
ip address 172.16.1.1 255.255.255.0
exit
router ospf 1
network 10.42.0.0 0.0.255.255 area 0
exit
router bgp 65001
bgp router-id 1.1.1.1
neighbor 10.42.12.2 remote-as 65001
network 172.16.1.0 mask 255.255.255.0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.42.12.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.42.23.1 255.255.255.252
no shutdown
interface g0/2
ip address 10.42.24.1 255.255.255.252
no shutdown
exit
router ospf 1
network 10.42.0.0 0.0.255.255 area 0
exit
router bgp 65001
bgp router-id 2.2.2.2
neighbor 10.42.12.1 remote-as 65001
neighbor 10.42.23.2 remote-as 65001
neighbor 10.42.24.2 remote-as 65001
end
```

```console R3
enable
configure terminal
interface g0/1
ip address 10.42.23.2 255.255.255.252
no shutdown
interface loopback 3
ip address 172.16.3.1 255.255.255.0
exit
router ospf 1
network 10.42.0.0 0.0.255.255 area 0
exit
router bgp 65001
bgp router-id 3.3.3.3
neighbor 10.42.23.1 remote-as 65001
network 172.16.3.0 mask 255.255.255.0
end
```

```console R4
enable
configure terminal
interface g0/1
ip address 10.42.24.2 255.255.255.252
no shutdown
interface loopback 4
ip address 172.16.4.1 255.255.255.0
exit
router ospf 1
network 10.42.0.0 0.0.255.255 area 0
exit
router bgp 65001
bgp router-id 4.4.4.4
neighbor 10.42.24.1 remote-as 65001
network 172.16.4.0 mask 255.255.255.0
end
```

```lab
sim.advance {"seconds": 90}
```

R2 has a session with each of the others, so it learns all three prefixes. A router never passes
a path it learned from an internal neighbour to another internal neighbour (that is how iBGP avoids
loops), so R2 keeps them to itself and no leaf hears about another leaf:

```verify
R2# show ip bgp
  expect: 172.16.1.0/24
  expect: 172.16.3.0/24
  expect: 172.16.4.0/24
R3# show ip bgp
  expect: 172.16.3.0/24
  reject: 172.16.1.0/24
  reject: 172.16.4.0/24
```

The usual cure is a session between every pair of routers. That is three sessions here and a lot
more in a real network. A route reflector is the other cure.

## Part 2 — One client

A route reflector is allowed to pass an internal path on to other internal neighbours. Which ones
depends on who it came from:

| The path came from | The reflector passes it to |
|---|---|
| a client | every internal neighbour, clients and non-clients |
| a non-client | the clients only |
| an external neighbour, or the reflector itself | every neighbour, as always |

Make R1 the only client of R2:

```console R2
configure terminal
router bgp 65001
neighbor 10.42.12.1 route-reflector-client
end
```

```lab
sim.advance {"seconds": 5}
```

R1 is a client, so what R1 sends is reflected to R3 and R4. R3 and R4 are not clients, so what they
send is reflected to R1 only: R3 never hears about R4 and R4 never hears about R3.

```verify
R1# show ip bgp
  expect: 172.16.3.0/24
  expect: 172.16.4.0/24
R3# show ip bgp
  expect: 172.16.1.0/24
  reject: 172.16.4.0/24
R4# show ip bgp
  expect: 172.16.1.0/24
  reject: 172.16.3.0/24
R2# show ip bgp neighbors 10.42.12.1
  expect: route-reflector-client
```

R3 has the path in its table, with the two attributes the reflector added. ORIGINATOR_ID is the
router id of the router that first told the AS about the path, and the CLUSTER_LIST names each
reflector it went through. Without `bgp cluster-id` a reflector uses its router id as the cluster
id:

```verify
R3# show ip bgp 172.16.1.0
  expect: originator 1.1.1.1, cluster list: 2.2.2.2
  expect: internal
R3# show ip route
  expect: 172.16.1.0/24
  expect: (BGP)
```

## Part 3 — Every leaf a client

Make R3 and R4 clients too. Now every path comes from a client and goes to every other leaf:

```console R2
configure terminal
router bgp 65001
neighbor 10.42.23.2 route-reflector-client
neighbor 10.42.24.2 route-reflector-client
end
```

```lab
sim.advance {"seconds": 5}
```

```verify
R3# show ip bgp
  expect: 172.16.1.0/24
  expect: 172.16.4.0/24
R4# show ip bgp
  expect: 172.16.1.0/24
  expect: 172.16.3.0/24
R3# show ip bgp 172.16.4.0
  expect: originator 4.4.4.4, cluster list: 2.2.2.2
```

Three sessions did what a full mesh of six would have done. The leaves know nothing about
reflection: it is all on R2.

## Part 4 — Name the cluster

A reflector and its clients form a cluster. The cluster id identifies it in the CLUSTER_LIST; give
it one that does not depend on a router id, so another reflector can share it later:

```console R2
configure terminal
router bgp 65001
bgp cluster-id 10.10.10.10
end
```

```lab
sim.advance {"seconds": 5}
```

```verify
R3# show ip bgp 172.16.1.0
  expect: cluster list: 10.10.10.10
R2# show ip bgp summary
  expect: Route reflector for cluster 10.10.10.10, 3 clients
R2# show running-config
  expect: bgp cluster-id 10.10.10.10
  expect: neighbor 10.42.12.1 route-reflector-client
  expect: neighbor 10.42.23.2 route-reflector-client
  expect: neighbor 10.42.24.2 route-reflector-client
```

## Part 5 — Loops

The two attributes are what makes reflection safe. A router drops an internal path whose
ORIGINATOR_ID is its own router id, and a reflector drops one whose CLUSTER_LIST holds its own
cluster id. R2 reflects R3's path to R1, R1 to R3, and so on, but nothing is told about its own
prefix again:

```verify
R1# show ip bgp 172.16.1.0
  expect: 1 path
  expect: originated here
R3# show ip bgp 172.16.3.0
  expect: 1 path
  expect: originated here
```

A second reflector with the same cluster id, serving the same clients, would drop the copy of
a path the first one reflected, so the clients would not hold two copies of every prefix. With
different cluster ids each reflector would take the other's copy, and the clients would see both.

## Part 6 — Take the reflector away

Without `route-reflector-client` on any neighbour, R2 is an ordinary iBGP router again:

```console R2
configure terminal
router bgp 65001
no neighbor 10.42.12.1 route-reflector-client
no neighbor 10.42.23.2 route-reflector-client
no neighbor 10.42.24.2 route-reflector-client
no bgp cluster-id
end
```

```lab
sim.advance {"seconds": 5}
```

```verify
R3# show ip bgp
  expect: 172.16.3.0/24
  reject: 172.16.1.0/24
  reject: 172.16.4.0/24
R2# show running-config
  reject: route-reflector-client
  reject: bgp cluster-id
```

## What to remember

- iBGP does not pass on what an internal neighbour sent. A full mesh of sessions, or route
  reflectors, is how every router learns every prefix.
- A reflector passes a client's paths to everyone and a non-client's paths to the clients only. The
  clients need nothing configured.
- ORIGINATOR_ID and CLUSTER_LIST never leave the AS; they stop a path from coming back to the
  router that started it or to a cluster that already carried it.
- A reflector does not change a path's next hop, so the clients must still reach it, here through
  OSPF.
