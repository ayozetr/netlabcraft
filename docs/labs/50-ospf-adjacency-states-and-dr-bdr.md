# Lab 50 — OSPF adjacency states and the DR and BDR

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 40 minutes

## Objectives

- Watch a neighbour walk through the states Init, 2-Way, ExStart, Exchange, Loading and Full
  with `debug ip ospf adj`, and see the packets that make each step (hello, database
  description, link-state request, update and acknowledgement) in the event list.
- Choose the designated router (DR) and backup (BDR) of a shared segment with
  `ip ospf priority`, and read why two routers that are neither stay in 2-Way.

## Topology

Four routers share one segment through a switch.

```text
 R1 ──┐
 R2 ──┤
      S1
 R3 ──┤
 R4 ──┘     10.50.0.0/24
```

| Device | Model | Interface | Address | Router ID | Priority |
|---|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.50.0.1/24 | 1.1.1.1 | 200 |
| R2 | Router-2 | G0/0 | 10.50.0.2/24 | 2.2.2.2 | 100 |
| R3 | Router-2 | G0/0 | 10.50.0.3/24 | 3.3.3.3 | 1 |
| R4 | Router-2 | G0/0 | 10.50.0.4/24 | 4.4.4.4 | 1 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
device.add {"model": "Router-2", "name": "R4"}
device.add {"model": "Switch-24", "name": "S1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "R2", "a_port": "g0/0", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "R3", "a_port": "g0/0", "b": "S1", "b_port": "fa0/3"}
link.add {"a": "R4", "a_port": "g0/0", "b": "S1", "b_port": "fa0/4"}
```

## Part 1 — Switch on the reports first

R3 reports each neighbour's states before OSPF starts:

```console R3
enable
debug ip ospf adj
```

## Part 2 — Configure OSPF

```console R1
enable
configure terminal
interface g0/0
ip address 10.50.0.1 255.255.255.0
ip ospf priority 200
no shutdown
exit
router ospf 1
router-id 1.1.1.1
network 10.50.0.0 0.0.0.255 area 0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.50.0.2 255.255.255.0
ip ospf priority 100
no shutdown
exit
router ospf 1
router-id 2.2.2.2
network 10.50.0.0 0.0.0.255 area 0
end
```

```console R3
configure terminal
interface g0/0
ip address 10.50.0.3 255.255.255.0
no shutdown
exit
router ospf 1
router-id 3.3.3.3
network 10.50.0.0 0.0.0.255 area 0
end
```

```console R4
enable
configure terminal
interface g0/0
ip address 10.50.0.4 255.255.255.0
no shutdown
exit
router ospf 1
router-id 4.4.4.4
network 10.50.0.0 0.0.0.255 area 0
end
```

The switch holds its ports back for spanning tree before it forwards, so give the hellos time:

```lab
sim.advance {"seconds": 120}
```

## Part 3 — Who is adjacent to whom

R1 has the highest priority and is the DR, R2 is the BDR. R3 is fully adjacent with both of
them, and only 2-Way with R4, because neither is the DR or the BDR:

```verify
R3# show ip ospf neighbor
  expect: FULL/DR
  expect: FULL/BDR
  expect: 2WAY/OTHER
R1# show ip ospf neighbor
  expect: FULL/OTHER
R3# show ip ospf interface g0/0
  expect: DR 10.50.0.1, BDR 10.50.0.2
```

## Part 4 — The states in the log

```verify
R3# show logging
  expect: Down -> Init
  expect: Init -> 2-Way
  expect: 2-Way -> ExStart
  expect: ExStart -> Exchange
  expect: Loading -> Full
```

The same exchange is in the Simulation event list:

```verify
events: OSPF
```

## Reflection

1. Why does R3 not go beyond 2-Way with R4?
2. Which of the five packet types does a router that has nothing to ask for skip?
3. What would change if R4 had priority 255 and OSPF were restarted with `clear ip ospf process`?
