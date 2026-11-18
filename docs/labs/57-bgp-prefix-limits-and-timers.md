# Lab 57 — BGP prefix limits and timers

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 30 minutes

## Objectives

- Protect a router from a neighbour that sends too many prefixes with
  `neighbor A.B.C.D maximum-prefix N`, and see the session end when the limit is passed.
- Use `warning-only` when the limit should only be a threshold, never a reason to drop the session.
- Set the keepalive interval and hold time of every neighbour at once with `timers bgp`, and
  read the negotiated values in `show ip bgp neighbors`.

## Topology

```text
 R1 ───────────── R2
 AS 65001         AS 65002
 Lo1-3 172.16.x.1  10.57.12.0/24
```

| Device | Model | Interface | Address | Runs |
|---|---|---|---|---|
| R1 | Router-2 | G0/1 | 10.57.12.1/24 | BGP AS 65001 |
| R1 | | Loopback1 to 3 | 172.16.1.1/24 to 172.16.3.1/24 | three networks |
| R2 | Router-2 | G0/1 | 10.57.12.2/24 | BGP AS 65002 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
```

```console R1
enable
configure terminal
interface g0/1
ip address 10.57.12.1 255.255.255.0
no shutdown
interface loopback 1
ip address 172.16.1.1 255.255.255.0
interface loopback 2
ip address 172.16.2.1 255.255.255.0
interface loopback 3
ip address 172.16.3.1 255.255.255.0
exit
router bgp 65001
bgp router-id 1.1.1.1
neighbor 10.57.12.2 remote-as 65002
network 172.16.1.0 mask 255.255.255.0
network 172.16.2.0 mask 255.255.255.0
network 172.16.3.0 mask 255.255.255.0
end
```

```console R2
enable
configure terminal
interface g0/1
ip address 10.57.12.2 255.255.255.0
no shutdown
exit
router bgp 65002
bgp router-id 2.2.2.2
neighbor 10.57.12.1 remote-as 65001
end
```

```lab
sim.advance {"seconds": 10}
```

R2 learns the three networks:

```verify
R2# show ip bgp
  expect: 172.16.1.0/24
  expect: 172.16.2.0/24
  expect: 172.16.3.0/24
```

## Part 1 — A limit that is passed

A router that takes a full routing table from a customer by mistake can run out of memory. A
prefix limit makes the mistake end the session instead. Allow R1 two prefixes only:

```console R2
configure terminal
router bgp 65002
neighbor 10.57.12.1 maximum-prefix 2
end
```

```lab
sim.advance {"seconds": 5}
```

R1 sends three, so the limit is passed. The session ends at both ends and R2 forgets everything
it learned from R1, even the first two prefixes:

```verify
R2# show ip bgp
  reject: 172.16.1.0/24
  reject: 172.16.3.0/24
R1# show ip bgp neighbors 10.57.12.2 advertised-routes
  expect: not Established
```

The session stays down until the limit is raised or removed.

## Part 2 — A limit that only warns

```console R2
configure terminal
router bgp 65002
neighbor 10.57.12.1 maximum-prefix 2 warning-only
end
```

```lab
sim.advance {"seconds": 5}
```

With `warning-only` the session is never ended, and all three prefixes are back:

```verify
R2# show ip bgp
  expect: 172.16.1.0/24
  expect: 172.16.3.0/24
R2# show ip bgp neighbors 10.57.12.1 routes
  expect: 172.16.2.0/24
R1# show ip bgp neighbors 10.57.12.2 advertised-routes
  expect: 172.16.3.0/24
```

## Part 3 — Timers for the whole process

A neighbour that has no `timers` line of its own runs with the keepalive interval and hold time of
the BGP process, 60 and 180 seconds unless `timers bgp` says otherwise:

```verify
R1# show ip bgp neighbors 10.57.12.2
  expect: Hold time 180 s, keepalive every 60 s
```

```console R1
configure terminal
router bgp 65001
timers bgp 10 30
end
```

```console R2
configure terminal
router bgp 65002
timers bgp 10 30
end
```

```lab
sim.advance {"seconds": 5}
```

```verify
R1# show ip bgp neighbors 10.57.12.2
  expect: Hold time 30 s, keepalive every 10 s
R1# show running-config
  expect: timers bgp 10 30
```

A `neighbor … timers` line still wins over the process: with `neighbor 10.57.12.2 timers 5 15` on R1 the
hold time becomes 15 seconds, the smaller of what the two ends ask for.

## Checklist

- [ ] The session ended when R1 sent more prefixes than the limit and came back with `warning-only`.
- [ ] `show ip bgp neighbors A.B.C.D routes` listed what was accepted from R1.
- [ ] `timers bgp 10 30` showed in the running configuration and in the neighbour view.
