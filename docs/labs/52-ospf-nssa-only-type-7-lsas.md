# Lab 52 — OSPF NSSA: keeping a type 7 route inside the area with nssa-only

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 40 minutes

## Objectives

- Read the P bit of a type 7 LSA and see which border router translates it into a type 5 LSA.
- Redistribute a route with `nssa-only` so its type 7 LSA has the P bit clear and stays inside
  the not-so-stubby area.
- Take the option away and watch the route reach the rest of the network again.

## Topology

R1 is in area 0. R2 and R4 are border routers between area 0 and the NSSA (area 1). R3 is inside
the NSSA and redistributes a static route. The three NSSA routers share a switch.

```text
       R1 ──────────┐
      /  \          │ area 0
    R2    R4 ───────┘
     \    /
      S1 ── R3        area 1 (NSSA)
```

| Device | Model | Interface | Address | Area | Router ID |
|---|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.52.0.1/24 | 0 | 1.1.1.1 |
| R1 | | G0/1 | 10.52.2.1/24 | 0 | |
| R2 | Router-2 | G0/0 | 10.52.0.2/24 | 0 | 2.2.2.2 |
| R2 | | G0/1 | 10.52.1.2/24 | 1 | |
| R3 | Router-2 | G0/0 | 10.52.1.3/24 | 1 | 3.3.3.3 |
| R4 | Router-2 | G0/0 | 10.52.2.4/24 | 0 | 4.4.4.4 |
| R4 | | G0/1 | 10.52.1.4/24 | 1 | |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
device.add {"model": "Router-2", "name": "R4"}
device.add {"model": "Switch-24", "name": "S1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R4", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "R3", "a_port": "g0/0", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "R4", "a_port": "g0/1", "b": "S1", "b_port": "fa0/3"}
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.52.0.1 255.255.255.0
no shutdown
exit
interface g0/1
ip address 10.52.2.1 255.255.255.0
no shutdown
exit
router ospf 1
router-id 1.1.1.1
network 10.52.0.0 0.0.0.255 area 0
network 10.52.2.0 0.0.0.255 area 0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.52.0.2 255.255.255.0
no shutdown
exit
interface g0/1
ip address 10.52.1.2 255.255.255.0
no shutdown
exit
router ospf 1
router-id 2.2.2.2
network 10.52.0.0 0.0.0.255 area 0
network 10.52.1.0 0.0.0.255 area 1
area 1 nssa
end
```

```console R4
enable
configure terminal
interface g0/0
ip address 10.52.2.4 255.255.255.0
no shutdown
exit
interface g0/1
ip address 10.52.1.4 255.255.255.0
no shutdown
exit
router ospf 1
router-id 4.4.4.4
network 10.52.2.0 0.0.0.255 area 0
network 10.52.1.0 0.0.0.255 area 1
area 1 nssa
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.52.1.3 255.255.255.0
no shutdown
exit
ip route 192.168.50.0 255.255.255.0 10.52.1.99
router ospf 1
router-id 3.3.3.3
network 10.52.1.0 0.0.0.255 area 1
redistribute static subnets
area 1 nssa
end
```

```lab
sim.advance {"seconds": 120}
```

## Part 1 — Translated

R3's static route is a type 7 LSA with the P bit set. The border router with the highest router
ID, R4, translates it into a type 5 LSA, so R1 learns the route as an external route:

```verify
R3# show ip ospf database nssa-external
  expect: P bit set
  expect: 4.4.4.4
R1# show ip ospf database external
  expect: 192.168.50.0
```

## Part 2 — Kept inside the area

```console R3
configure terminal
router ospf 1
redistribute static subnets nssa-only
end
```

```lab
sim.advance {"seconds": 30}
```

The type 7 LSA now has the P bit clear. Nobody translates it, and R1 has no type 5 LSA for it:

```verify
R3# show ip ospf database nssa-external
  expect: P bit clear
R3# show running-config
  expect: redistribute static subnets nssa-only
R1# show ip ospf database external
  reject: 192.168.50.0
```

## Part 3 — Translated again

```console R3
configure terminal
router ospf 1
redistribute static subnets
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ip ospf database external
  expect: 192.168.50.0
```

## Reflection

1. Which router translated the route, and why that one?
2. The area also has a default route sent by `default-information-originate`; why does adding
   `nssa-only` to it matter less than adding it to `redistribute`?
