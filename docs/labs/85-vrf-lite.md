# Lab 85 — VRF-lite: two networks on one pair of routers

**Course:** CCNA 3 follow-up (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Define two VRFs on a router, give each a route distinguisher, and put interfaces in them with
  `ip vrf forwarding`.
- Use the same addresses in both VRFs and see that they never meet: each VRF has its own routing
  table and its own ARP table.
- Route inside a VRF with a static route (`ip route vrf`) and with OSPF (`router ospf N vrf NAME`).
- Read a VRF's table with `show ip route vrf`, its neighbours with `show ip arp vrf`, and test it with `ping vrf`.

## Topology

```text
 PC1 ── R1 ══ two cables ══ R2 ── PC2
        g0/1  RED, 10.85.0.0/30   g0/1
        g0/2  BLUE, 10.85.0.0/30  g0/2
```

| Device | Model | Interface | VRF | Address |
|---|---|---|---|---|
| R1 | Router-3 | G0/0 | RED | 10.85.1.1/24 |
| R1 | | G0/1 | RED | 10.85.0.1/30 |
| R1 | | G0/2 | BLUE | 10.85.0.1/30 |
| R1 | | Loopback1 | BLUE | 1.1.1.1/32 |
| R2 | Router-3 | G0/0 | RED | 10.85.3.1/24 |
| R2 | | G0/1 | RED | 10.85.0.2/30 |
| R2 | | G0/2 | BLUE | 10.85.0.2/30 |
| R2 | | Loopback1 | BLUE | 2.2.2.2/32 |
| PC1 | PC | Fa0 | | 10.85.1.10/24, gateway 10.85.1.1 |
| PC2 | PC | Fa0 | | 10.85.3.10/24, gateway 10.85.3.1 |

The two cables between the routers carry the same subnet, 10.85.0.0/30. That is only possible
because each cable belongs to a different VRF.

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-3", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/2", "b": "R2", "b_port": "g0/2"}
```

```console PC1
ipconfig 10.85.1.10 255.255.255.0 10.85.1.1
```

```console PC2
ipconfig 10.85.3.10 255.255.255.0 10.85.3.1
```

## Part 1 — The VRFs and their interfaces

`ip vrf NAME` defines the VRF and opens its mode. The route distinguisher makes the VRF's prefixes
unique; route targets say what a VRF would export and import (they are kept and shown, VRF-lite
does not use them). `ip vrf forwarding` takes the address off the interface, so type the address after it.

```console R1
enable
configure terminal
ip vrf RED
rd 65085:1
route-target both 65085:1
exit
ip vrf BLUE
rd 65085:2
exit
interface g0/0
ip vrf forwarding RED
ip address 10.85.1.1 255.255.255.0
no shutdown
interface g0/1
ip vrf forwarding RED
ip address 10.85.0.1 255.255.255.252
no shutdown
interface g0/2
ip vrf forwarding BLUE
ip address 10.85.0.1 255.255.255.252
no shutdown
interface loopback 1
ip vrf forwarding BLUE
ip address 1.1.1.1 255.255.255.255
end
```

```console R2
enable
configure terminal
ip vrf RED
rd 65085:1
route-target both 65085:1
exit
ip vrf BLUE
rd 65085:2
exit
interface g0/0
ip vrf forwarding RED
ip address 10.85.3.1 255.255.255.0
no shutdown
interface g0/1
ip vrf forwarding RED
ip address 10.85.0.2 255.255.255.252
no shutdown
interface g0/2
ip vrf forwarding BLUE
ip address 10.85.0.2 255.255.255.252
no shutdown
interface loopback 1
ip vrf forwarding BLUE
ip address 2.2.2.2 255.255.255.255
end
```

```verify
R1# show ip vrf
  expect: RED
  expect: 65085:1
  expect: BLUE
R1# show ip route
  reject: 10.85.0.0/30
  reject: 10.85.1.0/24
R1# show ip route vrf RED
  expect: Routing Table: RED
  expect: 10.85.1.0/24
R1# show ip route vrf BLUE
  expect: 1.1.1.1/32
  reject: 10.85.1.0/24
```

The global table has none of these networks: an interface in a VRF belongs to that VRF's table only.

## Part 2 — A static route in RED

RED needs a route to the LAN behind the other router. The route is added to the RED table.

```console R1
configure terminal
ip route vrf RED 10.85.3.0 255.255.255.0 10.85.0.2
end
```

```console R2
configure terminal
ip route vrf RED 10.85.1.0 255.255.255.0 10.85.0.1
end
```

```verify
R1# ping vrf RED 10.85.0.2
  expect: !!!!
R1# show ip route vrf RED
  expect: 10.85.3.0/24
R1# show ip arp vrf RED
  expect: 10.85.0.2
R1# ping vrf RED 10.85.3.10
  expect: !!!!
```

The pings of the two PCs go through R1 and R2 in the RED tables:

```verify
ping PC1 10.85.3.10 => success
```

## Part 3 — The same address in two VRFs

R1 reaches 10.85.0.2 in BLUE through the other cable, and its ARP table for BLUE is its own.

```verify
R1# ping vrf BLUE 10.85.0.2
  expect: !!!!
R1# show ip arp vrf BLUE
  expect: 10.85.0.2
  reject: 10.85.1.10
R1# ping vrf BLUE 10.85.3.10
  reject: !!!
```

BLUE has no route to the LAN behind R2: a VRF reaches only what its own table knows. Shut the RED
cable down and RED stops while BLUE goes on:

```console R1
configure terminal
interface g0/1
shutdown
end
```

```verify
R1# ping vrf RED 10.85.0.2
  reject: !!!
R1# ping vrf BLUE 10.85.0.2
  expect: !!!!
```

```console R1
configure terminal
interface g0/1
no shutdown
end
```

## Part 4 — OSPF inside BLUE

BLUE runs its own OSPF process on both routers. The process is named after the VRF.

```console R1
configure terminal
router ospf 85 vrf BLUE
network 10.85.0.0 0.0.0.3 area 0
network 1.1.1.1 0.0.0.0 area 0
end
```

```console R2
configure terminal
router ospf 85 vrf BLUE
network 10.85.0.0 0.0.0.3 area 0
network 2.2.2.2 0.0.0.0 area 0
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
R1# show ip ospf neighbor vrf BLUE
  expect: 2.2.2.2
R1# show ip route vrf BLUE
  expect: 2.2.2.2/32
R1# ping vrf BLUE 2.2.2.2
  expect: !!!!
R1# show ip route
  reject: 2.2.2.2
R1# show ip route vrf RED
  reject: 2.2.2.2
R1# show running-config
  expect: router ospf 85 vrf BLUE
  expect: ip route vrf RED 10.85.3.0 255.255.255.0 10.85.0.2
```

The route to 2.2.2.2 is in BLUE only. RED and the global table do not have it, and a ping of
`2.2.2.2` from RED finds no route.

## Questions

1. Why can both cables use 10.85.0.0/30?
2. What does `ip vrf forwarding` do to an address that was already on the interface?
3. A third VRF is needed on the same cables: what would you add on each router?
