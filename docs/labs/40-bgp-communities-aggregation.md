# Lab 40 — BGP communities, aggregation and timers

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 60 minutes

## Objectives

- Tag a path with a community (`set community`) and see why a neighbour only receives it with
  `send-community`.
- Read communities in both formats (`ip bgp-community new-format`) and list the paths that carry
  one (`show ip bgp community`).
- Act on a community: a community list and `match community` in a route map.
- Keep a path inside its autonomous system with the well-known `no-export` community.
- Summarise two prefixes with `aggregate-address … summary-only as-set`.
- Give a neighbour its own keepalive and hold times (`neighbor … timers`).

## Topology

```text
 AS 65001                  AS 65002                  AS 65003
   R1 ─────────────────────── R2 ─────────────────────── R3
   10.40.12.0/30                    10.40.23.0/30
```

| Device | Model | Interface | Address | BGP router ID |
|---|---|---|---|---|
| R1 | Router-2 | G0/1 | 10.40.12.1/30 | 1.1.1.1 |
| R1 | | Loopback1 | 172.16.1.1/24 | |
| R1 | | Loopback2 | 172.16.2.1/24 | |
| R2 | Router-3 | G0/0 | 10.40.12.2/30 | 2.2.2.2 |
| R2 | | G0/1 | 10.40.23.1/30 | |
| R3 | Router-2 | G0/1 | 10.40.23.2/30 | 3.3.3.3 |

R1 owns two sites, `172.16.1.0/24` and `172.16.2.0/24`, and advertises both. R2 is its provider
and R3 is R2's customer further on.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-3", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R3", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
```

## Part 1 — Addresses and sessions

```console R1
enable
configure terminal
interface g0/1
ip address 10.40.12.1 255.255.255.252
no shutdown
interface loopback 1
ip address 172.16.1.1 255.255.255.0
interface loopback 2
ip address 172.16.2.1 255.255.255.0
exit
router bgp 65001
bgp router-id 1.1.1.1
neighbor 10.40.12.2 remote-as 65002
network 172.16.1.0 mask 255.255.255.0
network 172.16.2.0 mask 255.255.255.0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.40.12.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.40.23.1 255.255.255.252
no shutdown
exit
router bgp 65002
bgp router-id 2.2.2.2
neighbor 10.40.12.1 remote-as 65001
neighbor 10.40.23.2 remote-as 65003
end
```

```console R3
enable
configure terminal
interface g0/1
ip address 10.40.23.2 255.255.255.252
no shutdown
exit
router bgp 65003
bgp router-id 3.3.3.3
neighbor 10.40.23.1 remote-as 65002
end
```

```lab
sim.advance {"seconds": 10}
```

```verify
R3# show ip bgp
  expect: 172.16.1.0/24
  expect: 172.16.2.0/24
  expect: 65002 65001 i
```

## Part 2 — Tag a path with a community

A community is a 32-bit label a path carries. Write it as `AA:NN` (usually the AS number and a
number of your choosing). A route map sets it on the way out; here only the first site is tagged:

```console R1
configure terminal
ip prefix-list SITE-A permit 172.16.1.0/24
route-map TAG-OUT permit 10
match ip address prefix-list SITE-A
set community 65001:100 additive
exit
route-map TAG-OUT permit 20
exit
router bgp 65001
neighbor 10.40.12.2 route-map TAG-OUT out
end
```

```lab
sim.advance {"seconds": 2}
```

The community is set, but communities are only attached to what a router advertises when the
neighbour has `send-community`; without it R2 sees nothing:

```verify
R2# show ip bgp 172.16.1.0
  reject: communities:
```

```console R1
configure terminal
router bgp 65001
neighbor 10.40.12.2 send-community
end
```

```lab
sim.advance {"seconds": 2}
```

By default a community is shown as one number: 65001 times 65536, plus 100. The new format
shows `AA:NN`:

```verify
R2# show ip bgp 172.16.1.0
  expect: communities: 4259905636
  expect: local preference 100
R2# show ip bgp 172.16.2.0
  reject: communities:
```

```console R2
configure terminal
ip bgp-community new-format
end
```

```verify
R2# show ip bgp 172.16.1.0
  expect: communities: 65001:100
R2# show ip bgp community 65001:100
  expect: 172.16.1.0/24
  reject: 172.16.2.0/24
R2# show ip bgp community 65001:999
  expect: no path carries 65001:999
```

## Part 3 — Act on a community

A community list names the communities to look for, and `match community` puts it in a route
map. R2 prefers what R1 tagged:

```console R2
configure terminal
ip community-list standard SITE-A-ROUTES permit 65001:100
route-map FROM-R1 permit 10
match community SITE-A-ROUTES
set local-preference 300
exit
route-map FROM-R1 permit 20
exit
router bgp 65002
neighbor 10.40.12.1 route-map FROM-R1 in
end
```

```verify
R2# show ip bgp 172.16.1.0
  expect: local preference 300
R2# show ip bgp 172.16.2.0
  expect: local preference 100
R2# show ip community-list
  expect: Community list SITE-A-ROUTES (standard): 1 line
  expect: permit 65001:100
R2# show ip bgp community-list SITE-A-ROUTES
  expect: 172.16.1.0/24
  reject: 172.16.2.0/24
R2# show route-map FROM-R1
  expect: match community list: SITE-A-ROUTES
```

The second clause has no `match` line, so every other path passes unchanged, with the default
preference of 100.

## Part 4 — Keep a path inside the autonomous system

The well-known communities have a meaning to every router: `no-export` is never advertised to an
external neighbour, `no-advertise` to nobody, `local-AS` never leaves the local AS (there are no
confederations here, so it acts like `no-export`). Right now R3 sees both sites. Change R1's tag
to `no-export`, and let R2 send communities to R3 too:

```console R1
configure terminal
route-map TAG-OUT permit 10
set community no-export
end
```

```console R2
configure terminal
router bgp 65002
neighbor 10.40.23.2 send-community
end
```

```lab
sim.advance {"seconds": 2}
```

R2 holds the path and knows why it will not pass it on; R3 only has the other site:

```verify
R2# show ip bgp community no-export
  expect: 172.16.1.0/24
R2# show ip bgp 172.16.1.0
  expect: communities: no-export
R3# show ip bgp
  reject: 172.16.1.0/24
  expect: 172.16.2.0/24
```

## Part 5 — Summarise with an aggregate

R2 can advertise one summary for R1's sites. `aggregate-address` makes a path of its own while
at least one more specific path exists in the BGP table; `summary-only` stops the specific
ones from being advertised, and `as-set` records the autonomous systems they came through so
the summary does not hide a loop.

```console R2
configure terminal
router bgp 65002
aggregate-address 172.16.0.0 255.255.252.0 summary-only as-set
end
```

```lab
sim.advance {"seconds": 2}
```

```verify
R3# show ip bgp
  expect: 172.16.0.0/22
  expect: 65002 {65001} i
  reject: 172.16.2.0/24
R2# show ip bgp 172.16.2.0
  expect: not advertised: an aggregate with summary-only covers it
R2# show ip bgp 172.16.0.0 255.255.252.0
  expect: originated here
  expect: AS set {65001}
R2# show running-config
  expect: aggregate-address 172.16.0.0 255.255.252.0 summary-only as-set
```

R2 still has both specific prefixes in its own table; only what it tells its neighbours changed.
R2 also puts a discard route for the aggregate in its routing table, so traffic to an address that
no more specific prefix covers is dropped there instead of following a default route:

```verify
R2# show ip route
  expect: 172.16.0.0/22
  expect: summary, discarded on Null0 (BGP)
```

## Part 6 — Timers

A BGP session sends a keepalive every 60 seconds and gives up on a silent neighbour after 180
seconds. `neighbor … timers KEEPALIVE HOLD` changes both for one neighbour; the two routers
agree on the smaller hold time, and keepalives are never sent less often than a third of it.

```console R1
configure terminal
router bgp 65001
neighbor 10.40.12.2 timers 10 30
end
```

```verify
R1# show ip bgp neighbors 10.40.12.2
  expect: State: Established
  expect: Hold time 30 s, keepalive every 10 s
  expect: timers 10 30
R1# show running-config
  expect: neighbor 10.40.12.2 timers 10 30
  expect: neighbor 10.40.12.2 send-community
```

With a short hold time, a session whose path is lost (a link three hops away, say, behind an
`ebgp-multihop` neighbour) stays Established until the hold time runs out, and then drops with
`Down hold timer expired` in the log. A directly connected external neighbour drops at once when
its interface goes down.
