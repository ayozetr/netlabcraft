# Lab 31 — Reading the OSPF database and the routing summaries

**Course:** CCNA 3 (Enterprise Networking, Security, and Automation) · **Time:** 50 minutes

## Objectives

- Read each kind of link-state advertisement in `show ip ospf database router`, `network`,
  `summary` and `external`, and say which router originated it and why.
- Use `show ip route summary` and `show ip protocols summary` as a one-screen health check.
- Find a fault from what is missing in the database rather than from the neighbour list.

## Topology

```text
 PC1 ── G0/0  R1  G0/1 ──── G0/0  R2  G0/1 ──── G0/0  R3  G0/1 ── PC3
 area 0       10.31.12.0/24   area 0 | area 1   10.31.23.0/24      area 1
```

| Device | Model | Interface | Address | Router ID |
|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 | 1.1.1.1 |
| R1 | | G0/1 | 10.31.12.1/24 | |
| R2 | Router-2 | G0/0 | 10.31.12.2/24 | 2.2.2.2 |
| R2 | | G0/1 | 10.31.23.2/24 | |
| R3 | Router-2 | G0/0 | 10.31.23.3/24 | 3.3.3.3 |
| R3 | | G0/1 | 192.168.3.1/24 | |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw 192.168.1.1 | |
| PC3 | PC | Fa0 | 192.168.3.10/24, gw 192.168.3.1 | |

R2 is an area border router: its G0/0 is in area 0 and its G0/1 in area 1. R3 also has a static
route to 172.31.0.0/16, which it redistributes into OSPF.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC3", "b": "R3", "b_port": "g0/1"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC3
ipconfig 192.168.3.10 255.255.255.0 192.168.3.1
```

## Part 1 — Build the two areas

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.31.12.1 255.255.255.0
no shutdown
exit
router ospf 1
router-id 1.1.1.1
network 192.168.1.0 0.0.0.255 area 0
network 10.31.12.0 0.0.0.255 area 0
passive-interface g0/0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.31.12.2 255.255.255.0
no shutdown
interface g0/1
ip address 10.31.23.2 255.255.255.0
no shutdown
exit
router ospf 1
router-id 2.2.2.2
network 10.31.12.0 0.0.0.255 area 0
network 10.31.23.0 0.0.0.255 area 1
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.31.23.3 255.255.255.0
no shutdown
interface g0/1
ip address 192.168.3.1 255.255.255.0
no shutdown
exit
ip route 172.31.0.0 255.255.0.0 10.31.23.99
router ospf 1
router-id 3.3.3.3
network 10.31.23.0 0.0.0.255 area 1
network 192.168.3.0 0.0.0.255 area 1
passive-interface g0/1
redistribute static subnets
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
ping PC1 192.168.3.10 => success
R1# show ip ospf neighbor
  expect: 2.2.2.2
  expect: FULL
```

## Part 2 — Read the database

R1 is in area 0 only. Its database has three kinds of advertisement that matter, and a fourth
for the redistributed route.

**Router LSAs (type 1)** describe a router and its links in one area. R1 holds one for each
router in area 0, which are R1 and R2:

```verify
R1# show ip ospf database router
  expect: Router link states, area 0
  expect: Router 1.1.1.1
  expect: Router 2.2.2.2
  expect: stub network 192.168.1.0/24
  expect: transit network, designated router
  reject: Router 3.3.3.3
```

R3 is not in R1's database as a router: it is in area 1, and a router LSA never leaves its
area. Ask for one router by its link-state ID:

```verify
R1# show ip ospf database router 2.2.2.2
  expect: Router 2.2.2.2
  reject: Router 1.1.1.1
R2# show ip ospf database router
  expect: Router link states, area 0
  expect: Router link states, area 1
  expect: Router 3.3.3.3
```

**Network LSAs (type 2)** come from the designated router of each broadcast segment, and list
the routers attached to it:

```verify
R1# show ip ospf database network
  expect: Network 10.31.12.0/24, designated router
  expect: attached routers: 1.1.1.1, 2.2.2.2
```

**Summary LSAs (type 3)** are how an area border router tells one area about networks in
another. R1 knows area 1's networks only through these, with the metric R2 advertises:

```verify
R1# show ip ospf database summary
  expect: Summary link states, area 0
  expect: Network 10.31.23.0/24
  expect: Network 192.168.3.0/24
  expect: advertised by 2.2.2.2
```

**External LSAs (type 5)** carry the routes redistributed from outside OSPF, and flood to every
area, so R1 has the one R3 originated:

```verify
R1# show ip ospf database external
  expect: Network 172.31.0.0/16
  expect: advertised by 3.3.3.3, type 2 metric 20
```

The routing table follows the database. Count the routes by where they came from:

```verify
R1# show ip route summary
  expect: connected
  expect: OSPF
  expect: inter-area 2
  expect: external 1
R1# show ip protocols summary
  expect: OSPF
  expect: 1.1.1.1
```

## Part 3 — A fault that leaves the neighbours alone

Somebody makes R2's area 1 interface passive "to save bandwidth":

```console R2
configure terminal
router ospf 1
passive-interface g0/1
end
```

```lab
sim.advance {"seconds": 120}
```

## Part 4 — Diagnose from the database

```verify
ping PC1 192.168.3.10 => failure
R1# show ip ospf neighbor
  expect: 2.2.2.2
  expect: FULL
```

R1 still has its neighbour R2 and says nothing is wrong. The database and the summaries say what
is:

```verify
R1# show ip ospf database summary
  reject: 192.168.3.0
  expect: Network 10.31.23.0/24
R1# show ip route summary
  expect: inter-area 1
  expect: external 0
R1# show ip ospf database external
  reject: 172.31.0.0
R2# show ip protocols summary
  expect: OSPF
R2# show ip ospf neighbor
  reject: 3.3.3.3
```

R2 no longer sends hellos on its area-1 link, so R3 is not a neighbour and its advertisements
age out: the summary for 192.168.3.0/24 and the external route are gone from R1's database, and
R1 has no route to them. R2 still advertises the network of the passive link itself
(10.31.23.0/24), which is why one inter-area route is left. R2's neighbour list shows it, and
`show ip ospf interface g0/1` says why:

```verify
R2# show ip ospf interface g0/1
  expect: Passive
```

Repair it:

```console R2
configure terminal
router ospf 1
no passive-interface g0/1
end
```

```lab
sim.advance {"seconds": 90}
```

```verify
ping PC1 192.168.3.10 => success
R1# show ip ospf database summary
  expect: Network 192.168.3.0/24
R1# show ip ospf database external
  expect: Network 172.31.0.0/16
R1# show ip route summary
  expect: inter-area 2
R2# show ip ospf neighbor
  expect: 3.3.3.3
```

## Reflection

1. Which advertisement kinds would R3 hold that R1 does not, and which does R1 hold that R3
   does not?
2. Why did R1's neighbour list stay the same during the fault while its database changed?
3. R1 reaches 172.31.0.0/16 with metric 20 whichever path it takes. Where in the output is that
   metric, and why does it not change with the number of hops?
