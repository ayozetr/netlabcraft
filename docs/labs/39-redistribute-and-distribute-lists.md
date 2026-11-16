# Lab 39 — Redistribute between routing protocols and filter with a distribute list

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 75 minutes

## Objectives

- Explain why two routing protocols on the same router do not share routes until a
  `redistribute` line says so.
- Redistribute RIP into EIGRP (`redistribute rip metric …`) and EIGRP into RIP
  (`redistribute eigrp … ` with `default-metric`), and say why a metric is needed.
- Read the result in the routing table: `D EX` routes, their administrative distance of 170, and
  the hop count RIP gives a route it was handed.
- Explain why a protocol is never handed back the routes it gave away.
- Choose what is redistributed with a prefix list and a route map.
- Filter what a router installs (`distribute-list … in`) or advertises (`distribute-list … out`),
  and find the filters in `show ip protocols` and the running configuration.

## Topology

```text
 RIP island                      EIGRP 100 island
 PC1 ── R1 ─────────── R2 ─────────── R3 ── PC3
 192.168.1.0/24  10.39.12.0/24   172.31.23.0/24   192.168.3.0/24
```

| Device | Model | Interface | Address | Runs |
|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 | RIP |
| R1 | | G0/1 | 10.39.12.1/24 | RIP |
| R1 | | Loopback1 | 172.16.1.1/24 | RIP |
| R1 | | Loopback2 | 172.16.2.1/24 | RIP |
| R2 | Router-3 | G0/0 | 10.39.12.2/24 | RIP |
| R2 | | G0/1 | 172.31.23.2/24 | EIGRP 100 |
| R3 | Router-2 | G0/1 | 172.31.23.3/24 | EIGRP 100 |
| R3 | | G0/0 | 192.168.3.1/24 | EIGRP 100 |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw 192.168.1.1 | |
| PC3 | PC | Fa0 | 192.168.3.10/24, gw 192.168.3.1 | |

R2 sits between two routing domains: RIP towards R1, EIGRP towards R3. It is the only router
that will redistribute.

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

## Part 1 — Two islands

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.39.12.1 255.255.255.0
no shutdown
interface loopback 1
ip address 172.16.1.1 255.255.255.0
interface loopback 2
ip address 172.16.2.1 255.255.255.0
exit
router rip
version 2
network 192.168.1.0
network 10.0.0.0
network 172.16.0.0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.39.12.2 255.255.255.0
no shutdown
interface g0/1
ip address 172.31.23.2 255.255.255.0
no shutdown
exit
router rip
version 2
network 10.0.0.0
exit
router eigrp 100
network 172.31.23.0 0.0.0.255
end
```

```console R3
enable
configure terminal
interface g0/1
ip address 172.31.23.3 255.255.255.0
no shutdown
interface g0/0
ip address 192.168.3.1 255.255.255.0
no shutdown
exit
router eigrp 100
network 172.31.23.0 0.0.0.255
network 192.168.3.0 0.0.0.255
end
```

```lab
sim.advance {"seconds": 30}
```

R2 runs both protocols, and each protocol only knows what its own neighbours told it. R1 has
never heard of R3's LAN and R3 has never heard of R1's:

```verify
R1# show ip route rip
  reject: 192.168.3.0
R3# show ip route eigrp
  reject: 192.168.1.0
ping PC1 192.168.3.10 => failure
```

## Part 2 — RIP into EIGRP

A route learned by one protocol is handed to another with `redistribute`. EIGRP needs five
numbers for a route it did not compute itself: the bandwidth (kbit/s), the delay (tens of
microseconds), the reliability, the load and the MTU. Only the bandwidth and the delay enter the
metric. Without them the routes are not advertised, and the console tells you so:

```console R2
configure terminal
router eigrp 100
redistribute rip
end
```

```verify
R2# show ip protocols
  expect: Redistributing: rip
R3# show ip route eigrp
  reject: 192.168.1.0
```

Give the redistribution its metric:

```console R2
configure terminal
router eigrp 100
redistribute rip metric 100000 100 255 1 1500
end
```

```lab
sim.advance {"seconds": 30}
```

R3 now has R1's networks as external EIGRP routes (`D EX`), with administrative distance 170
instead of 90, because EIGRP did not compute them. R2's own connected network 10.39.12.0 is not
a RIP route, so it is not among them:

```verify
R3# show ip route eigrp
  expect: D EX 192.168.1.0/24
  expect: D EX 172.16.1.0/24
  expect: D EX 172.16.2.0/24
  expect: [170/
  reject: 10.39.12.0
R2# show running-config
  expect: redistribute rip metric 100000 100 255 1 1500
ping PC1 192.168.3.10 => failure
```

The ping still fails: PC3 receives the request and answers, but R1 has no route back to
192.168.3.0.

## Part 3 — EIGRP into RIP, and a default metric

RIP's metric is a hop count from 1 to 15; 16 means unreachable. A route redistributed into RIP
with no metric would be unreachable, so none is advertised until a metric is named, on the line
or for every line with `default-metric`:

```console R2
configure terminal
router rip
redistribute eigrp 100
end
```

```verify
R1# show ip route rip
  reject: 192.168.3.0
```

```console R2
configure terminal
router rip
default-metric 4
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ip route rip
  expect: R    192.168.3.0/24
  expect: [120/4]
R2# show ip protocols
  expect: Default redistribution metric: 4
  expect: Redistributing: eigrp 100
ping PC1 192.168.3.10 => success
ping PC3 192.168.1.10 => success
```

Redistribution takes routes from the routing table, so it hands on only what each protocol
installed. R2 learned R1's networks through RIP, so they are RIP routes there, and the EIGRP
copy it created for R3 is not offered back to RIP:

```verify
R2# show ip route
  expect: R    192.168.1.0/24
  reject: D EX 192.168.1.0
```

## Part 4 — Choose what is redistributed with a route map

R3 does not need R1's second loopback. A prefix list names it, a route map uses the list, and
the `redistribute` line names the map. A route a route map clause does not permit is dropped.

```console R2
configure terminal
ip prefix-list LO2 permit 172.16.2.0/24
route-map RIP-TO-EIGRP deny 10
match ip address prefix-list LO2
exit
route-map RIP-TO-EIGRP permit 20
exit
router eigrp 100
redistribute rip metric 100000 100 255 1 1500 route-map RIP-TO-EIGRP
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R3# show ip route eigrp
  expect: D EX 172.16.1.0/24
  expect: D EX 192.168.1.0/24
  reject: 172.16.2.0
R2# show ip protocols
  expect: rip (route-map RIP-TO-EIGRP)
ping PC1 192.168.3.10 => success
```

## Part 5 — Distribute lists

A distribute list filters what a routing process installs (`in`) or advertises (`out`), either
for every interface or for one. It names a prefix list, an access list or (for `in`) a route
map. On R3, install nothing for 172.16.1.0 with a standard access list:

```console R3
configure terminal
access-list 5 deny 172.16.1.0 0.0.0.255
access-list 5 permit any
router eigrp 100
distribute-list 5 in
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R3# show ip route eigrp
  reject: 172.16.1.0
  expect: D EX 192.168.1.0/24
R3# show ip protocols
  expect: Incoming route filter: access list 5 on every interface
R3# show running-config
  expect: distribute-list 5 in
```

On R2, keep R3's LAN out of the RIP updates sent to R1, with a prefix list on one interface:

```console R2
configure terminal
ip prefix-list NO-LAN3 deny 192.168.3.0/24
ip prefix-list NO-LAN3 permit 0.0.0.0/0 le 32
router rip
distribute-list prefix NO-LAN3 out g0/0
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ip route rip
  reject: 192.168.3.0
R2# show ip protocols
  expect: Outgoing route filter: prefix list NO-LAN3 on Gi0/0
R2# show running-config
  expect: distribute-list prefix NO-LAN3 out GigabitEthernet0/0
ping PC1 192.168.3.10 => failure
```

Take the filters away and the routes come back:

```console R2
configure terminal
router rip
no distribute-list prefix NO-LAN3 out g0/0
end
```

```console R3
configure terminal
router eigrp 100
no distribute-list 5 in
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ip route rip
  expect: 192.168.3.0/24
R3# show ip route eigrp
  expect: D EX 172.16.1.0/24
ping PC1 192.168.3.10 => success
```

A filter that names a prefix list, access list or route map that does not exist lets every route
through, the same as for a BGP neighbour. When a route is missing, check the names with
`show ip prefix-list`, `show access-lists` and `show route-map`.
