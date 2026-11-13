# Lab 23 — Troubleshooting: OSPF neighbours that will not form

**Course:** CCNA 3 (Enterprise Networking, Security, and Automation) · **Time:** 50 minutes

## Objectives

- Start from an OSPFv2 network that works, then break three things in it: a timer, an area and
  a passive interface.
- Tell the three apart from `show ip ospf neighbor`, `show ip ospf interface` and
  `show ip protocols`, and from what is missing in `show ip route ospf`.
- Repair each one and prove the whole network converged.

## Topology

```text
 PC1 ── G0/0  R1  G0/1 ──── G0/1  R2  G0/2 ──── G0/1  R3  G0/0 ── PC3
                10.23.12.0/30         10.23.23.0/30
```

| Device | Model | Interface | Address | Router ID |
|---|---|---|---|---|
| R1 | Router-3 | G0/0 | 192.168.1.1/24 | 1.1.1.1 |
| R1 | | G0/1 | 10.23.12.1/30 | |
| R2 | Router-3 | G0/1 | 10.23.12.2/30 | 2.2.2.2 |
| R2 | | G0/2 | 10.23.23.1/30 | |
| R3 | Router-3 | G0/1 | 10.23.23.2/30 | 3.3.3.3 |
| R3 | | G0/0 | 192.168.3.1/24 | |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw 192.168.1.1 | |
| PC3 | PC | Fa0 | 192.168.3.10/24, gw 192.168.3.1 | |

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-3", "name": "R2"}
device.add {"model": "Router-3", "name": "R3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "R2", "a_port": "g0/2", "b": "R3", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC3", "b": "R3", "b_port": "g0/0"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC3
ipconfig 192.168.3.10 255.255.255.0 192.168.3.1
```

## Part 1 — The working network

All three routers are in area 0. The LAN interfaces are passive: they are advertised but form
no neighbours.

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.23.12.1 255.255.255.252
no shutdown
exit
router ospf 1
router-id 1.1.1.1
network 192.168.1.0 0.0.0.255 area 0
network 10.23.12.0 0.0.0.3 area 0
passive-interface g0/0
end
```

```console R2
enable
configure terminal
interface g0/1
ip address 10.23.12.2 255.255.255.252
no shutdown
interface g0/2
ip address 10.23.23.1 255.255.255.252
no shutdown
exit
router ospf 1
router-id 2.2.2.2
network 10.23.12.0 0.0.0.3 area 0
network 10.23.23.0 0.0.0.3 area 0
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 192.168.3.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.23.23.2 255.255.255.252
no shutdown
exit
router ospf 1
router-id 3.3.3.3
network 192.168.3.0 0.0.0.255 area 0
network 10.23.23.0 0.0.0.3 area 0
passive-interface g0/0
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
R2# show ip ospf neighbor
  expect: 1.1.1.1
  expect: 3.3.3.3
  expect: FULL
R1# show ip route ospf
  expect: 192.168.3.0/24
ping PC1 192.168.3.10 => success
```

## Part 2 — The faults

Three changes are made behind your back.

```console R2
configure terminal
interface g0/1
ip ospf hello-interval 5
exit
router ospf 1
no network 10.23.23.0 0.0.0.3 area 0
network 10.23.23.0 0.0.0.3 area 1
end
```

```console R1
configure terminal
router ospf 1
passive-interface g0/1
end
```

```lab
sim.advance {"seconds": 120}
```

## Part 3 — Diagnose

```verify
ping PC1 192.168.3.10 => failure
R2# show ip ospf neighbor
  reject: FULL
R1# show ip route ospf
  reject: 192.168.3.0
```

No neighbour is full on R2. There are three links to look at, and each has its own cause.

### R1 and R2

```verify
R1# show ip ospf neighbor
  reject: 2.2.2.2
R1# show ip protocols
  expect: Passive interfaces
  expect: Gi0/1
R1# show ip ospf interface g0/1
  expect: Passive: advertised
```

R1's link to R2 is passive: R1 advertises the network but sends no Hello. Remove the passive
setting on that interface only.

```console R1
configure terminal
router ospf 1
no passive-interface g0/1
end
```

R1 still does not become a neighbour of R2. R2's Hello interval was changed:

```verify
R2# show ip ospf interface g0/1
  expect: Hello every 5 s
R1# show ip ospf interface g0/1
  expect: Hello every 10 s
```

Neighbours must agree on the Hello and dead intervals (RFC 2328). Put R2 back to the default:

```console R2
configure terminal
interface g0/1
no ip ospf hello-interval
end
```

### R2 and R3

```verify
R2# show ip ospf interface g0/2
  expect: area 1
R3# show ip ospf interface g0/1
  expect: area 0
```

The two ends of a link must be in the same area. R2's `network` statement put the link in
area 1. Move it back:

```console R2
configure terminal
router ospf 1
no network 10.23.23.0 0.0.0.3 area 1
network 10.23.23.0 0.0.0.3 area 0
end
```

## Part 4 — Prove the repair

```lab
sim.advance {"seconds": 90}
```

```verify
R2# show ip ospf neighbor
  expect: 1.1.1.1
  expect: 3.3.3.3
  expect: FULL
R2# show ip ospf neighbor 3.3.3.3
  expect: FULL
R2# show ip ospf neighbor g0/1
  expect: 1.1.1.1
R1# show ip route 192.168.3.0 255.255.255.0
  expect: O
R1# show ip route ospf
  expect: 192.168.3.0/24
R3# show ip route ospf
  expect: 192.168.1.0/24
ping PC1 192.168.3.10 => success
ping PC3 192.168.1.10 => success
PC1> tracert 192.168.3.10
  expect: 192.168.1.1
  expect: 10.23.12.2
  expect: 10.23.23.2
```

## Reflection

1. Which of the three faults leaves the neighbour list empty, and which leaves a neighbour stuck in a state short of FULL?
2. Why is it right for the LAN interfaces to be passive, and wrong for the links between routers?
3. Which `show` command would have told you about all three without looking at each interface?
