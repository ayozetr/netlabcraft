# Lab 37 — Filter routes with a prefix list and a route map

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 60 minutes

## Objectives

- Write a prefix list with `ip prefix-list`, using `ge` and `le`, and read it with
  `show ip prefix-list`.
- Filter what an eBGP neighbour sends with `neighbor … prefix-list … in`.
- Write a route map with `match` and `set` lines and apply it to a neighbour with
  `neighbor … route-map … in`, to steer traffic with a higher local preference.
- Explain the two implicit denies: a prefix no prefix-list line matches, and a route no
  route-map clause matches, are dropped.
- Explain what happens when a neighbour names a list that does not exist.

## Topology

```text
 AS 65001                  AS 65002                  AS 65003
   R1 ─────────────────────── R2 ─────────────────────── R3
   10.37.12.0/30                    10.37.23.0/30
```

| Device | Model | Interface | Address | BGP router ID |
|---|---|---|---|---|
| R1 | Router-2 | G0/1 | 10.37.12.1/30 | 1.1.1.1 |
| R1 | | Loopback1 | 203.0.113.1/24 | |
| R1 | | Loopback2 | 198.51.100.1/24 | |
| R1 | | Loopback3 | 10.250.0.1/16 | |
| R2 | Router-3 | G0/0 | 10.37.12.2/30 | 2.2.2.2 |
| R2 | | G0/1 | 10.37.23.1/30 | |
| R3 | Router-2 | G0/1 | 10.37.23.2/30 | 3.3.3.3 |
| R3 | | Loopback1 | 198.51.100.3/24 | |
| R3 | | Loopback2 | 203.0.113.3/24 | |

R2 is a customer with two providers, R1 and R3. Both providers can reach `203.0.113.0/24` and
`198.51.100.0/24`; R1 also advertises `10.250.0.0/16`, an address range that should never be
accepted from a provider. R2 will filter and prefer routes with policy.

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
ip address 10.37.12.1 255.255.255.252
no shutdown
interface loopback 1
ip address 203.0.113.1 255.255.255.0
interface loopback 2
ip address 198.51.100.1 255.255.255.0
interface loopback 3
ip address 10.250.0.1 255.255.0.0
exit
router bgp 65001
bgp router-id 1.1.1.1
neighbor 10.37.12.2 remote-as 65002
network 203.0.113.0 mask 255.255.255.0
network 198.51.100.0 mask 255.255.255.0
network 10.250.0.0 mask 255.255.0.0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.37.12.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.37.23.1 255.255.255.252
no shutdown
exit
router bgp 65002
bgp router-id 2.2.2.2
neighbor 10.37.12.1 remote-as 65001
neighbor 10.37.23.2 remote-as 65003
end
```

```console R3
enable
configure terminal
interface g0/1
ip address 10.37.23.2 255.255.255.252
no shutdown
interface loopback 1
ip address 198.51.100.3 255.255.255.0
interface loopback 2
ip address 203.0.113.3 255.255.255.0
exit
router bgp 65003
bgp router-id 3.3.3.3
neighbor 10.37.23.1 remote-as 65002
network 198.51.100.0 mask 255.255.255.0
network 203.0.113.0 mask 255.255.255.0
end
```

Sessions need a few seconds of simulated time.

```lab
sim.advance {"seconds": 10}
```

With no policy R2 accepts everything. The two providers both offer `198.51.100.0/24` with an AS
path of the same length, so BGP falls back to the lowest neighbour router ID: R1's path is best.

```verify
R2# show ip bgp 198.51.100.0
  expect: BGP entry for 198.51.100.0/24: 2 paths, the best is path 1
  expect: Path 1 (best): AS path 65001, next hop 10.37.12.1
  expect: Path 2: AS path 65003, next hop 10.37.23.2
R2# show ip bgp
  expect: 10.250.0.0/16
```

## Part 2 — A prefix list that drops what must not be accepted

A prefix list is an ordered list of `permit` and `deny` lines. A line without `ge` or `le`
matches exactly that prefix; `le 32` makes it match that prefix and everything inside it. The
first line that matches decides, and a prefix no line matches is denied. Lines are numbered in
steps of five when you do not type a sequence number.

```console R2
configure terminal
ip prefix-list FROM-R1 deny 10.0.0.0/8 le 32
ip prefix-list FROM-R1 permit 0.0.0.0/0 le 32
router bgp 65002
neighbor 10.37.12.1 prefix-list FROM-R1 in
end
```

```verify
R2# show ip prefix-list FROM-R1
  expect: Prefix list FROM-R1: 2 lines
  expect: seq 5 deny 10.0.0.0/8 le 32
  expect: seq 10 permit 0.0.0.0/0 le 32
R2# show ip bgp
  reject: 10.250.0.0/16
  expect: 203.0.113.0/24
R2# show ip bgp neighbors 10.37.12.1
  expect: Policy: inbound prefix list FROM-R1
  expect: Prefixes received 2
  expect: State: Established
```

The session never went down: the policy applies to the paths already there. Without the last
line (`permit 0.0.0.0/0 le 32`) the implicit deny would have dropped every prefix, not just the
private range.

## Part 3 — A route map that prefers one provider

A route map is a list of clauses tried in order of sequence number. A clause with no `match`
line matches every route; `set` lines change the attributes of what the clause permits. Give the
routes from R3 a higher local preference than the default of 100:

```console R2
configure terminal
route-map PREFER-R3 permit 10
set local-preference 200
exit
router bgp 65002
neighbor 10.37.23.2 route-map PREFER-R3 in
end
```

```verify
R2# show ip bgp 198.51.100.0
  expect: Path 1 (best): AS path 65003, next hop 10.37.23.2
  expect: local preference 200
R2# show ip route bgp
  expect: B    198.51.100.0/24
  expect: via 10.37.23.2, GigabitEthernet0/1 (BGP) [20/0]
R2# show route-map PREFER-R3
  expect: Route map PREFER-R3: 1 clause
  expect: permit, sequence 10
  expect: set local-preference 200
R2# show ip bgp neighbors 10.37.23.2
  expect: Policy: inbound route map PREFER-R3
```

Local preference is compared before the length of the AS path and the router ID, so R3 now
wins for every prefix both providers offer.

## Part 4 — The route map's implicit deny

Make the clause match only one prefix. First a prefix list for it:

```console R2
configure terminal
ip prefix-list WEB permit 198.51.100.0/24
route-map PREFER-R3 permit 10
match ip address prefix-list WEB
end
```

Now `203.0.113.0/24` from R3 matches no clause, and a route no clause matches is dropped. Only
R1's path to it is left:

```verify
R2# show ip bgp 203.0.113.0
  expect: BGP entry for 203.0.113.0/24: 1 path
  expect: AS path 65001
  reject: AS path 65003
R2# show ip bgp 198.51.100.0
  expect: Path 1 (best): AS path 65003
```

Add a second clause with no `match` line to let everything else through unchanged:

```console R2
configure terminal
route-map PREFER-R3 permit 20
end
```

```verify
R2# show route-map PREFER-R3
  expect: Route map PREFER-R3: 2 clauses
  expect: match prefix list: WEB
  expect: every route matches
R2# show ip bgp 203.0.113.0
  expect: BGP entry for 203.0.113.0/24: 2 paths
```

## Part 5 — The configuration, and a name with no list behind it

The policy is part of the running configuration, and `clear ip bgp * soft` is accepted for the
habit of applying policy again; here policy applies as soon as it changes, so nothing is reset.

```verify
R2# show running-config
  expect: neighbor 10.37.12.1 prefix-list FROM-R1 in
  expect: neighbor 10.37.23.2 route-map PREFER-R3 in
  expect: ip prefix-list FROM-R1 seq 5 deny 10.0.0.0/8 le 32
  expect: route-map PREFER-R3 permit 10
  expect: match ip address prefix-list WEB
R2# clear ip bgp * soft
  expect: No session was reset
```

Delete the list but leave the neighbour naming it:

```console R2
configure terminal
no ip prefix-list FROM-R1
end
```

A neighbour that names a list that does not exist lets everything through, so the private range
is accepted again, and the neighbour's detail view says why:

```verify
R2# show ip bgp
  expect: 10.250.0.0/16
R2# show ip bgp neighbors 10.37.12.1
  expect: not defined: lets everything through
```

This is the usual way a filter goes missing: a misspelt name. Check the names with
`show ip bgp neighbors` and `show ip prefix-list`.
