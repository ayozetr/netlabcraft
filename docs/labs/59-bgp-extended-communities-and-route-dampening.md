# Lab 59 — BGP extended communities and route dampening

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 60 minutes

## Objectives

- Tag the routes a router sends with extended communities (a route target and a site of origin)
  with `set extcommunity`, send them with `neighbor … send-community extended`, and read them
  with `show ip bgp A.B.C.D`.
- Match extended communities in a route map through an `ip extcommunity-list` and change what is
  done with the routes that match.
- Turn on route dampening with `bgp dampening`, make a route flap, and read the penalty:
  `show ip bgp flap-statistics`, `show ip bgp dampened-paths` and the `d` status code.
- See a held-down route come back by itself once its penalty has decayed, which takes simulated
  time, not commands.

## Topology

```text
 R1 ───────────── R2
 AS 65001         AS 65002
 Lo1 172.16.1.1   10.59.12.0/24
 Lo3 172.16.3.1
```

| Device | Model | Interface | Address | Runs |
|---|---|---|---|---|
| R1 | Router-2 | G0/1 | 10.59.12.1/24 | BGP AS 65001 |
| R1 | | Loopback1 | 172.16.1.1/24 | |
| R1 | | Loopback3 | 172.16.3.1/24 | |
| R2 | Router-2 | G0/0 | 10.59.12.2/24 | BGP AS 65002 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
```

```console R1
enable
configure terminal
interface g0/1
ip address 10.59.12.1 255.255.255.0
no shutdown
interface loopback 1
ip address 172.16.1.1 255.255.255.0
interface loopback 3
ip address 172.16.3.1 255.255.255.0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.59.12.2 255.255.255.0
no shutdown
end
```

## Part 1 — Tag the routes with extended communities

An extended community is eight octets: a type, a subtype and a value made of a global
administrator and a local number. A **route target** (`rt`) names the VPN a route belongs to; a
**site of origin** (`soo`) names where it came from. The global administrator is an AS number or
an IPv4 address. R1 gives 172.16.1.0/24 a route target and a site of origin, and the other
prefix a different route target, and sends them with `send-community extended` (the standard
`send-community` carries only the 32-bit communities).

```console R1
configure terminal
ip prefix-list ONE seq 5 permit 172.16.1.0/24
route-map TAG permit 10
match ip address prefix-list ONE
set extcommunity rt 65001:100 soo 10.59.12.1:5
exit
route-map TAG permit 20
set extcommunity rt 65001:300
exit
router bgp 65001
bgp router-id 1.1.1.1
neighbor 10.59.12.2 remote-as 65002
neighbor 10.59.12.2 route-map TAG out
neighbor 10.59.12.2 send-community extended
network 172.16.1.0 mask 255.255.255.0
network 172.16.3.0 mask 255.255.255.0
end
```

```console R2
configure terminal
router bgp 65002
bgp router-id 2.2.2.2
neighbor 10.59.12.1 remote-as 65001
end
```

```lab
sim.advance {"seconds": 8}
```

```verify
R2# show ip bgp 172.16.1.0
  expect: extended communities: RT:65001:100 SoO:10.59.12.1:5
R2# show ip bgp 172.16.3.0
  expect: extended communities: RT:65001:300
R1# show ip bgp neighbors 10.59.12.2
  expect: send-community extended
```

## Part 2 — Match them in a route map

R2 gives the routes that carry the route target 65001:100 a local preference of 300. The match
goes through an extended community list, as `match community` goes through a community list.

```console R2
configure terminal
ip extcommunity-list standard WANT permit rt 65001:100
route-map IN permit 10
match extcommunity WANT
set local-preference 300
exit
route-map IN permit 20
exit
router bgp 65002
neighbor 10.59.12.1 route-map IN in
end
```

```verify
R2# show ip extcommunity-list
  expect: Extended community list WANT (standard)
  expect: permit rt 65001:100
R2# show route-map IN
  expect: match extended community list: WANT
R2# show ip bgp
  expect: 300
```

## Part 3 — Dampen a flapping route

R2 now penalises the routes it learns from R1 when they flap. Each withdrawal costs 1000; the
penalty halves every 15 minutes; above 2000 the route is held down until the penalty has fallen
below 750.

```console R2
configure terminal
router bgp 65002
bgp dampening
end
```

```verify
R2# show ip bgp flap-statistics
  expect: no route has flapped
```

Make 172.16.1.0/24 flap: shut the loopback R1 gets the prefix from, look at R2, bring it up and
look again. R2 only sees a flap when it looks, so each step has a `show` after it.

```console R1
configure terminal
interface loopback 1
shutdown
end
```

```verify
R2# show ip bgp
  reject: 172.16.1.0/24
```

```console R1
configure terminal
interface loopback 1
no shutdown
end
```

```verify
R2# show ip bgp
  expect: 172.16.1.0/24
```

A second flap makes 2000, which does not pass the limit: the route is used, and the statistics
list it.

```console R1
configure terminal
interface loopback 1
shutdown
end
```

```verify
R2# show ip bgp
  reject: 172.16.1.0/24
```

```console R1
configure terminal
interface loopback 1
no shutdown
end
```

```verify
R2# show ip bgp flap-statistics
  expect: 172.16.1.0/24
R2# show ip bgp dampened-paths
  expect: no route is held down
R2# show ip route bgp
  expect: 172.16.1.0
```

The third flap passes the limit:

```console R1
configure terminal
interface loopback 1
shutdown
end
```

```verify
R2# show ip bgp
  reject: 172.16.1.0/24
```

```console R1
configure terminal
interface loopback 1
no shutdown
end
```

R1 advertises the prefix again, but R2 holds it down. It is still in the BGP table, marked `d`,
and has left the routing table; the other prefix, which never flapped, is untouched.

```verify
R2# show ip bgp 172.16.1.0
  expect: held down by dampening
R2# show ip bgp dampened-paths
  expect: 172.16.1.0/24
R2# show ip route bgp
  reject: 172.16.1.0
  expect: 172.16.3.0
```

## Part 4 — Wait for the penalty to decay

The penalty is about 3000 and halves every 15 minutes: after 30 minutes it is 750, the reuse
limit. Wait 31 simulated minutes.

```lab
sim.advance {"seconds": 1860}
```

```verify
R2# show ip bgp dampened-paths
  expect: no route is held down
R2# show ip route bgp
  expect: 172.16.1.0
R2# show ip bgp flap-statistics
  expect: 172.16.1.0/24
```

The route is back, and the history stays in the flap statistics until it has decayed too.
`clear ip bgp dampening` forgets it at once:

```console R2
clear ip bgp dampening
```

```verify
R2# show ip bgp flap-statistics
  expect: no route has flapped
```

## What you learned

- Extended communities travel only to a neighbour with `send-community extended` (or `both`);
  a route target and a site of origin are written `ASN:NN` or `A.B.C.D:NN`.
- `ip extcommunity-list` and `match extcommunity` filter on them like community lists do on
  standard communities.
- Dampening is a penalty that grows with every withdrawal and halves with time: a route is held
  down above the suppress limit, listed with `d` and not used, and returns by itself once the
  penalty has decayed below the reuse limit.
- The clock decides: the same flaps spread over hours never suppress a route.

## Checklist

- [ ] `show ip bgp 172.16.1.0` on R2 listed the route target and the site of origin.
- [ ] The route map with `match extcommunity` gave the tagged route a local preference of 300.
- [ ] The third flap held 172.16.1.0/24 down and the routing table lost it.
- [ ] After 31 simulated minutes the route was back without any command.
