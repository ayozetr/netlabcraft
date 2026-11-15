# Lab 32 — EIGRP: neighbours, metrics, passive interfaces, variance and summaries

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 75 minutes

## Objectives

- Turn on EIGRP in one autonomous system with `network` statements and router IDs, and read
  `show ip eigrp neighbors`, `topology` and `interfaces`.
- Work out a route's composite metric from the bandwidth and delay of the links it crosses, and
  watch a slower link change which path wins.
- Tell a successor from a feasible successor, and use `variance` to share traffic over a path
  that is not the best.
- Make LAN interfaces passive, and see what a passive interface does to a link between routers.
- Advertise two networks as one with `ip summary-address eigrp`.

## Topology

```text
                      R2 (Router-2)
                G0/0 /            \ G0/1
        10.32.12.0/30              10.32.23.0/30
                G0/0 /            \ G0/0
 PC1 ── G0/2  R1 ───────────────────── R3  G0/2 ── PC3
 192.168.1.0/24  G0/1  10.32.13.0/30  G0/1  192.168.3.0/24
                                       Lo1 172.16.0.1/24
                                       Lo2 172.16.1.1/24
```

| Device | Model | Interface | Address | Router ID |
|---|---|---|---|---|
| R1 | Router-3 | G0/0 | 10.32.12.1/30 | 1.1.1.1 |
| R1 | | G0/1 | 10.32.13.1/30 | |
| R1 | | G0/2 | 192.168.1.1/24 | |
| R2 | Router-2 | G0/0 | 10.32.12.2/30 | 2.2.2.2 |
| R2 | | G0/1 | 10.32.23.1/30 | |
| R3 | Router-3 | G0/0 | 10.32.23.2/30 | 3.3.3.3 |
| R3 | | G0/1 | 10.32.13.2/30 | |
| R3 | | G0/2 | 192.168.3.1/24 | |
| R3 | | Lo1, Lo2 | 172.16.0.1/24, 172.16.1.1/24 | |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw 192.168.1.1 | |
| PC3 | PC | Fa0 | 192.168.3.10/24, gw 192.168.3.1 | |

All three routers run EIGRP in autonomous system 10. The two loopbacks on R3 stand for two
small networks behind it, which Part 5 advertises as one.

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-3", "name": "R3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/0", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R3", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/2"}
link.add {"a": "PC3", "b": "R3", "b_port": "g0/2"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC3
ipconfig 192.168.3.10 255.255.255.0 192.168.3.1
```

## Part 1 — Turn EIGRP on

A `network` statement with a wildcard mask picks the interfaces whose address falls inside it;
those interfaces send hellos, form neighbours and have their networks advertised. Routers only
become neighbours inside the same autonomous system number.

```console R1
enable
configure terminal
interface g0/0
ip address 10.32.12.1 255.255.255.252
no shutdown
interface g0/1
ip address 10.32.13.1 255.255.255.252
no shutdown
interface g0/2
ip address 192.168.1.1 255.255.255.0
no shutdown
exit
router eigrp 10
eigrp router-id 1.1.1.1
network 10.32.0.0 0.0.255.255
network 192.168.1.0 0.0.0.255
no auto-summary
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.32.12.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.32.23.1 255.255.255.252
no shutdown
exit
router eigrp 10
eigrp router-id 2.2.2.2
network 10.32.0.0 0.0.255.255
no auto-summary
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.32.23.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.32.13.2 255.255.255.252
no shutdown
interface g0/2
ip address 192.168.3.1 255.255.255.0
no shutdown
interface loopback 1
ip address 172.16.0.1 255.255.255.0
interface loopback 2
ip address 172.16.1.1 255.255.255.0
exit
router eigrp 10
eigrp router-id 3.3.3.3
network 10.32.0.0 0.0.255.255
network 192.168.3.0 0.0.0.255
network 172.16.0.0 0.0.255.255
no auto-summary
end
```

```lab
sim.advance {"seconds": 30}
```

Every router should have two neighbours, and every network should be in every routing table
as a `D` route (EIGRP's code, administrative distance 90):

```verify
R1# show ip eigrp neighbors
  expect: EIGRP neighbours, AS 10
  expect: 10.32.12.2
  expect: 10.32.13.2
R2# show ip eigrp neighbors
  expect: 10.32.12.1
  expect: 10.32.23.2
R3# show ip eigrp neighbors
  expect: 10.32.23.1
  expect: 10.32.13.1
R1# show ip route eigrp
  expect: 192.168.3.0/24
  expect: 10.32.23.0/30
  expect: 172.16.0.0/24
  expect: (EIGRP) [90/
R3# show ip route eigrp
  expect: 192.168.1.0/24
  expect: 10.32.12.0/30
R1# show ip protocols
  expect: Routing protocol: EIGRP, autonomous system 10
  expect: Router id: 1.1.1.1
ping PC1 192.168.3.10 => success
ping PC3 192.168.1.10 => success
```

The `[90/3072]` in a route is the administrative distance and the metric. All the links so far
are Gigabit, so the direct link between R1 and R3 is the shortest way to R3's LAN:

```verify
R1# show ip route eigrp
  expect: [90/3072]
PC1> tracert 192.168.3.10
  expect: 192.168.1.1
  expect: 10.32.13.2
  reject: 10.32.12.2
```

## Part 2 — Passive interfaces

The LAN interfaces have no router behind them, so there is nobody to say hello to. Making an
interface passive stops its hellos but keeps its network in the advertisements:

```console R1
configure terminal
router eigrp 10
passive-interface g0/2
end
```

```console R3
configure terminal
router eigrp 10
passive-interface g0/2
end
```

```verify
R1# show ip eigrp interfaces
  expect: Gi0/2
  expect: 192.168.1.0/24
  expect: yes
R1# show ip protocols
  expect: Passive interfaces: Gi0/2
R3# show ip route eigrp
  reject: 192.168.3.0
R1# show ip route eigrp
  expect: 192.168.3.0/24
ping PC1 192.168.3.10 => success
```

R3's LAN is still in R1's table, so the passive interface kept its advertisement. The same
setting on a link between routers has a very different effect. Make R2's G0/0 passive:

```console R2
configure terminal
router eigrp 10
passive-interface g0/0
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R2# show ip eigrp neighbors
  expect: 10.32.23.2
  reject: 10.32.12.1
R1# show ip eigrp neighbors
  expect: 10.32.13.2
  reject: 10.32.12.2
ping PC1 192.168.3.10 => success
```

R1 and R2 are no longer neighbours: a passive interface neither sends hellos nor accepts a
neighbour, so a link between routers must never be passive. R1 still reaches everything over
R3. Put R2 back:

```console R2
configure terminal
router eigrp 10
no passive-interface g0/0
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ip eigrp neighbors
  expect: 10.32.12.2
  expect: 10.32.13.2
```

## Part 3 — Metrics, successors and feasible successors

EIGRP's metric (with its default weights) is `256 × (10 000 000 / slowest bandwidth in kbit/s +
total delay in tens of microseconds)`. The slowest bandwidth is the minimum along the path; the
delays add up. A Gigabit port counts as 1 000 000 kbit/s and 10 microseconds (1 unit), so one
Gigabit hop to R3's LAN costs `256 × (10 + 1 + 1) = 3072` and the way through R2 costs
`256 × (10 + 3) = 3328`.

The topology table lists, per destination, the feasible distance (FD, the best metric) and the
paths that qualify. Every path is shown as (distance through that neighbour / distance the
neighbour itself reports). With all links equal, R1 has one path to R3's LAN and no spare:

```verify
R1# show ip eigrp topology
  expect: EIGRP topology, AS 10, router id 1.1.1.1
  expect: 192.168.3.0/24  FD 3072, 1 successor
  expect: via 10.32.13.2, Gi0/1 (3072 / 2816)
  reject: feasible successor
```

R2's route to the same LAN has a metric of 3072 as well, which is not lower than R1's own FD,
so R2 does not qualify as a backup. DUAL's feasibility condition says a neighbour may be a
backup (a **feasible successor**) only if the distance it reports is lower than our current
FD; that is what guarantees the backup path does not run through us.

Now the direct link is made a 100 Mbit/s link. The `bandwidth` command does not change the
real speed; it tells EIGRP (and QoS) what to assume. Set it at both ends:

```console R1
configure terminal
interface g0/1
bandwidth 100000
end
```

```console R3
configure terminal
interface g0/1
bandwidth 100000
end
```

```lab
sim.advance {"seconds": 30}
```

Over the slow link the metric is `256 × (100 + 1 + 1) = 26112`, so R2 becomes the successor
with `3328` and the direct link is now a feasible successor: R3 reports `2816`, lower than
R1's FD of `3328`.

```verify
R1# show ip eigrp topology
  expect: 192.168.3.0/24  FD 3328, 1 successor
  expect: via 10.32.12.2, Gi0/0 (3328 / 3072)
  expect: feasible successor
  expect: via 10.32.13.2, Gi0/1 (26112 / 2816)
R1# show ip eigrp interfaces
  expect: 100000
R1# show ip route eigrp
  expect: [90/3328]
  reject: via 10.32.13.2
PC1> tracert 192.168.3.10
  expect: 10.32.12.2
  expect: 10.32.23.2
  reject: 10.32.13.2
ping PC1 192.168.3.10 => success
```

The topology table has the backup ready: if the R1–R2 link failed, R1 could switch to the
direct link at once, without asking anyone, because DUAL already knows it is loop-free.

## Part 4 — Variance

A feasible successor stays in the topology table and out of the routing table, unless
`variance` is set: a feasible successor whose distance is less than *variance × FD* is used
too, and traffic is shared in proportion to the metrics. R1's path over the slow link costs
26112 and the FD is 3328, so a variance of 8 (3328 × 8 = 26624) is the lowest that includes it.

```console R1
configure terminal
router eigrp 10
variance 8
end
```

```verify
R1# show ip protocols
  expect: variance 8
R1# show ip route eigrp
  expect: via 10.32.12.2
  expect: via 10.32.13.2
  expect: [90/3328]
  expect: [90/26112]
R1# show ip eigrp topology
  expect: in use (variance)
ping PC1 192.168.3.10 => success
```

The route to R3's LAN now has two next hops, with different metrics. Variance only admits
feasible successors, so it can never create a loop. R1's route to `10.32.23.0/30` over the
slow link (26112, against an FD of 3072) is outside 8 × 3072 and stays out. Put the variance
back before the last part:

```console R1
configure terminal
router eigrp 10
no variance
end
```

```verify
R1# show ip route eigrp
  reject: via 10.32.13.2
```

## Part 5 — Summarising two networks as one

R1 and R2 learn both loopback networks of R3 as separate `/24` routes. R3 can send one
`/23` out of its two uplinks instead. The summary is configured on the interface the routers
learn it through:

```verify
R1# show ip route eigrp
  expect: 172.16.0.0/24
  expect: 172.16.1.0/24
R2# show ip route eigrp
  expect: 172.16.0.0/24
```

```console R3
configure terminal
interface g0/0
ip summary-address eigrp 10 172.16.0.0 255.255.254.0
interface g0/1
ip summary-address eigrp 10 172.16.0.0 255.255.254.0
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ip route eigrp
  expect: 172.16.0.0/23
  reject: 172.16.0.0/24
  reject: 172.16.1.0/24
R2# show ip route eigrp
  expect: 172.16.0.0/23
  reject: 172.16.1.0/24
R3# show ip route eigrp
  expect: 172.16.0.0/23
  expect: summary, discarded on Null0 (EIGRP)
R3# show running-config interface g0/0
  expect: ip summary-address eigrp 10 172.16.0.0 255.255.254.0
ping PC1 172.16.0.1 => success
ping PC1 172.16.1.1 => success
ping PC3 192.168.1.10 => success
```

R3 keeps a route for the summary that points at `Null0`: traffic for an address inside the
`/23` that none of its real networks owns is dropped there instead of being sent back out
towards its sender. R1 and R2 have one route instead of two; with a hundred small networks
behind R3, that is the point.

## Reflection

1. Why did R2 not qualify as a feasible successor in Part 3 before the link became slow, even
   though its path was only slightly worse?
2. What would happen to the `variance 8` route if the slow link failed? And to the route
   without any variance?
3. Which two settings must two routers share to become EIGRP neighbours, and which of the
   Part 2 commands broke that on the link between R1 and R2?
4. Why is the summary configured on R3's two uplinks and not on R1 and R2?
