# Lab 14 — OSPFv2: single area, DR election and a second area

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 75 minutes

## Objectives

- Turn on OSPFv2 two ways: with `network` statements and with interface commands.
- Set router IDs, make LAN interfaces passive and change the reference bandwidth.
- Control the DR/BDR election on a shared segment with interface priorities.
- Change the hello and dead timers, and see that neighbours must agree on them.
- Add a second area behind an area border router, and share a default route.

## Topology

```text
                     ┌── (G0/0) R2 ── (G0/1) PC2's LAN
 PC1 ── (G0/1) R1 (G0/0) ── S1 ──┤
                     └── (G0/0) R3 (G0/1) ──── (G0/0) R4 (G0/1) ── PC4
           area 0: the S1 segment and the R1, R2 LANs    area 1: R3–R4 link and R4's LAN
```

| Device | Model | Interface | Address | Area | Router ID |
|---|---|---|---|---|---|
| R1 | Router-3 | G0/0 | 10.0.123.1/24 | 0 | 1.1.1.1 |
| R1 | | G0/1 | 192.168.1.1/24 | 0 | |
| R1 | | G0/2 | 203.0.113.1/30 (to the Internet) | — | |
| R2 | Router-2 | G0/0 | 10.0.123.2/24 | 0 | 2.2.2.2 |
| R2 | | G0/1 | 192.168.2.1/24 | 0 | |
| R3 | Router-2 | G0/0 | 10.0.123.3/24 | 0 | 3.3.3.3 |
| R3 | | G0/1 | 10.0.34.1/30 | 1 | |
| R4 | Router-2 | G0/0 | 10.0.34.2/30 | 1 | 4.4.4.4 |
| R4 | | G0/1 | 192.168.4.1/24 | 1 | |
| ISP | Router-2 | G0/0 | 203.0.113.2/30 | — | |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw .1 | | |
| PC2 | PC | Fa0 | 192.168.2.10/24, gw .1 | | |
| PC4 | PC | Fa0 | 192.168.4.10/24, gw .1 | | |

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
device.add {"model": "Router-2", "name": "R4"}
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC4"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "R2", "a_port": "g0/0", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "R3", "a_port": "g0/0", "b": "S1", "b_port": "fa0/3"}
link.add {"a": "R3", "a_port": "g0/1", "b": "R4", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/2", "b": "ISP", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/1"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/1"}
link.add {"a": "PC4", "b": "R4", "b_port": "g0/1"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC2
ipconfig 192.168.2.10 255.255.255.0 192.168.2.1
```

```console PC4
ipconfig 192.168.4.10 255.255.255.0 192.168.4.1
```

The ISP router stands for the Internet; it only knows the way back to the company:

```console ISP
enable
configure terminal
interface g0/0
ip address 203.0.113.2 255.255.255.252
no shutdown
exit
ip route 192.168.0.0 255.255.0.0 203.0.113.1
ip route 10.0.0.0 255.0.0.0 203.0.113.1
end
```

## Part 1 — Addresses and DR priorities

On the shared segment one router is elected **designated router** (DR) and one **backup**
(BDR); the others only become fully adjacent with those two. The highest interface priority
wins (then the highest router ID); priority 0 never takes part. R1 should be the DR, R3 the
BDR, and R2 neither.

```console R1
enable
configure terminal
interface g0/0
ip address 10.0.123.1 255.255.255.0
ip ospf priority 200
no shutdown
interface g0/1
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/2
ip address 203.0.113.1 255.255.255.252
no shutdown
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.0.123.2 255.255.255.0
ip ospf priority 0
no shutdown
interface g0/1
ip address 192.168.2.1 255.255.255.0
no shutdown
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.0.123.3 255.255.255.0
ip ospf priority 100
no shutdown
interface g0/1
ip address 10.0.34.1 255.255.255.252
no shutdown
end
```

```console R4
enable
configure terminal
interface g0/0
ip address 10.0.34.2 255.255.255.252
no shutdown
interface g0/1
ip address 192.168.4.1 255.255.255.0
no shutdown
end
```

## Part 2 — OSPF with network statements (R1, R3, R4)

A `network` statement takes an address and a wildcard mask: every interface whose address
matches joins the given area. R3 is the area border router: one interface in each area.

```console R1
configure terminal
router ospf 10
router-id 1.1.1.1
auto-cost reference-bandwidth 1000
network 10.0.123.0 0.0.0.255 area 0
network 192.168.1.0 0.0.0.255 area 0
passive-interface g0/1
exit
ip route 0.0.0.0 0.0.0.0 203.0.113.2
router ospf 10
default-information originate
end
```

```console R3
configure terminal
router ospf 10
router-id 3.3.3.3
auto-cost reference-bandwidth 1000
network 10.0.123.0 0.0.0.255 area 0
network 10.0.34.0 0.0.0.3 area 1
end
```

```console R4
configure terminal
router ospf 10
router-id 4.4.4.4
auto-cost reference-bandwidth 1000
network 10.0.34.0 0.0.0.3 area 1
network 192.168.4.0 0.0.0.255 area 1
passive-interface g0/1
end
```

## Part 3 — OSPF with interface commands (R2)

Instead of `network` statements, each interface can join an area itself:

```console R2
configure terminal
router ospf 10
router-id 2.2.2.2
auto-cost reference-bandwidth 1000
passive-interface g0/1
exit
interface g0/0
ip ospf 10 area 0
interface g0/1
ip ospf 10 area 0
end
```

On a shared segment the routers wait for the dead interval (40 s) before electing; give
them time to elect and exchange their databases:

```lab
sim.advance {"seconds": 60}
```

```verify
R2# show ip ospf neighbor
  expect: 1.1.1.1
  expect: FULL/DR
  expect: 3.3.3.3
  expect: FULL/BDR
R1# show ip ospf interface g0/0
  expect: priority 200: this router is the DR
  expect: DR 10.0.123.1, BDR 10.0.123.3
  expect: router ID 1.1.1.1
R2# show ip ospf interface g0/0
  expect: this router never becomes DR or BDR
R4# show ip ospf neighbor
  expect: 3.3.3.3
  expect: FULL
R4# show ip route
  expect: O IA 192.168.1.0/24
  expect: O IA 192.168.2.0/24
  expect: O*E2 0.0.0.0/0
R1# show ip route
  expect: O IA 192.168.4.0/24
R1# show ip protocols
  expect: 1.1.1.1
ping PC1 192.168.4.10 => success
ping PC2 192.168.4.10 => success
ping PC4 203.0.113.2 => success
```

## Part 4 — Timers

Lower the hello and dead intervals on R1's segment interface only. Neighbours must agree on
both, so R2 and R3 drop R1 once its dead interval passes:

```console R1
configure terminal
interface g0/0
ip ospf hello-interval 5
ip ospf dead-interval 20
end
```

```lab
sim.advance {"seconds": 45}
```

```verify
R1# show ip ospf interface g0/0
  expect: Hello every 5 s, dead after 20 s
  expect: Neighbours on this interface: 0
R2# show ip ospf neighbor
  reject: 1.1.1.1
ping PC2 192.168.1.10 => failure
```

Set the same timers on R2 and R3 and the adjacencies come back:

```console R2
configure terminal
interface g0/0
ip ospf hello-interval 5
ip ospf dead-interval 20
end
```

```console R3
configure terminal
interface g0/0
ip ospf hello-interval 5
ip ospf dead-interval 20
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
R2# show ip ospf neighbor
  expect: 1.1.1.1
  expect: FULL
ping PC2 192.168.1.10 => success
ping PC4 192.168.1.10 => success
```

## Reflection

1. Which routers does R2 become fully adjacent with, and why not with every router on the
   segment?
2. Why does R4's table show the area 0 networks as `O IA`, and the default route as `O*E2`?
3. Why change the reference bandwidth on every router, and not just one?
