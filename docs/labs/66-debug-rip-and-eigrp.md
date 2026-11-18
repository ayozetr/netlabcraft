# Lab 66 — Reading `debug` output: RIP updates and EIGRP hellos

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Use `debug ip rip` to see which networks a neighbour advertises, and find a router that
  does not advertise its LAN from what is missing in its neighbour's updates.
- Use `debug ip rip events` and `debug ip routing` to watch a route appear once the fault is
  repaired.
- Use `debug eigrp packets` to read the autonomous-system number in a neighbour's hellos, find
  an AS mismatch from it, and confirm the repair with `debug eigrp neighbors`.

## Topology

```text
 PC1 ── R1 ── R2 ── R3 ── PC3             R4 ── R5
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| R1 | | G0/1 | 10.66.12.1/30 |
| R2 | Router-2 | G0/0 | 10.66.12.2/30 |
| R2 | | G0/1 | 10.66.23.1/30 |
| R3 | Router-2 | G0/0 | 10.66.23.2/30 |
| R3 | | G0/1 | 192.168.3.1/24 |
| R4 | Router-2 | G0/0 | 10.66.45.1/30 |
| R5 | Router-2 | G0/0 | 10.66.45.2/30 |
| PC1 | PC | Fa0 | 192.168.1.10/24 |
| PC3 | PC | Fa0 | 192.168.3.10/24 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
device.add {"model": "Router-2", "name": "R4"}
device.add {"model": "Router-2", "name": "R5"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/0"}
link.add {"a": "R4", "a_port": "g0/0", "b": "R5", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC3", "b": "R3", "b_port": "g0/1"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC3
ipconfig 192.168.3.10 255.255.255.0 192.168.3.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.66.12.1 255.255.255.252
no shutdown
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.66.12.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.66.23.1 255.255.255.252
no shutdown
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.66.23.2 255.255.255.252
no shutdown
interface g0/1
ip address 192.168.3.1 255.255.255.0
no shutdown
end
```

```console R4
enable
configure terminal
interface g0/0
ip address 10.66.45.1 255.255.255.252
no shutdown
end
```

```console R5
enable
configure terminal
interface g0/0
ip address 10.66.45.2 255.255.255.252
no shutdown
end
```

## Part 1 — RIP: a LAN that nobody hears about

RIP version 2 runs on all three routers. R3's configuration forgets one `network` statement:
the one for its LAN. Watch from R1 first, with the debugging on before the fault is repaired.

```console R1
configure terminal
router rip
version 2
network 192.168.1.0
network 10.0.0.0
no auto-summary
end
debug ip rip events
debug ip routing
```

The first look at the debugging says what is true now, so let it run once before the other
routers are configured; everything after that is reported as it changes.

```lab
sim.advance {"seconds": 1}
```

```console R2
configure terminal
router rip
version 2
network 10.0.0.0
no auto-summary
end
```

```console R3
configure terminal
router rip
version 2
network 10.0.0.0
no auto-summary
end
```

```lab
sim.advance {"seconds": 90}
sim.advance {"seconds": 1}
```

```verify
ping PC1 192.168.3.10 => failure
R1# show ip route
  expect: 10.66.23.0/30
  reject: 192.168.3.0/24
```

R1 learned the link between R2 and R3 and not the LAN behind R3. The debugging tells the same
story as events as they happened: the route to the link appeared, and the LAN's route never did.

```verify
R1# show logging
  expect: RIP event debug: route 10.66.23.0/30 via 10.66.12.2 appeared
  expect: Routing debug: R route 10.66.23.0/30 via 10.66.12.2 appeared
  reject: route 192.168.3.0/24
```

To see which router should have advertised it, look at the update itself. `debug ip rip` prints
every update R1 hears, with the networks in it and their metrics:

```console R1
debug ip rip
clear logging
```

```lab
sim.advance {"seconds": 35}
```

```verify
R1# show logging
  expect: RIP debug: version 2 update from 10.66.12.2 on GigabitEthernet0/1
  expect: 10.66.23.0/30 metric 1
  reject: 192.168.3.0/24
```

R2 advertises what it knows, and it does not know the LAN: so R2 did not hear it from R3. Ask R3
what it sends instead of guessing: its running configuration lists the networks it
advertises.

```verify
R3# show running-config
  expect: network 10.0.0.0
  reject: network 192.168.3.0
```

Add the missing network and watch the update change.

```console R3
configure terminal
router rip
network 192.168.3.0
end
```

```console R1
clear logging
```

```lab
sim.advance {"seconds": 90}
sim.advance {"seconds": 1}
```

```verify
R1# show logging
  expect: 192.168.3.0/24 metric 2
  expect: RIP event debug: route 192.168.3.0/24 via 10.66.12.2 appeared
  expect: Routing debug: R route 192.168.3.0/24 via 10.66.12.2 appeared
ping PC1 192.168.3.10 => success
```

The metric is 2: R3's own network is one hop for R2, and R1 is one more hop away. Turn the
debugging off.

```console R1
undebug all
```

```verify
R1# show debugging
  expect: No debugging is on
```

## Part 2 — EIGRP: two routers in two autonomous systems

R4 and R5 are cabled and addressed and both run EIGRP, and they never become neighbours.

```console R4
configure terminal
router eigrp 10
network 10.66.45.0 0.0.0.3
end
```

```console R5
configure terminal
router eigrp 20
network 10.66.45.0 0.0.0.3
end
```

```lab
sim.advance {"seconds": 20}
```

```verify
R4# show ip eigrp neighbors
  expect: No EIGRP neighbours in AS 10
```

The neighbour table is empty, and it does not say why. The hellos do: turn on `debug eigrp
packets` and read the autonomous-system number in what R4 hears, and `debug eigrp neighbors` to
be told when a neighbour appears.

```console R4
debug eigrp packets
debug eigrp neighbors
```

```lab
sim.advance {"seconds": 1}
sim.advance {"seconds": 12}
```

```verify
R4# show logging
  expect: EIGRP debug: hello from 10.66.45.2 on GigabitEthernet0/0, AS 20
  reject: EIGRP neighbour debug: neighbour 10.66.45.2
```

R5 sends hellos, so the cable, the addresses and the `network` statement are all right. The hello
says AS 20, and R4 is in AS 10: a hello from another autonomous system is not a neighbour.
Move R5 into AS 10.

```console R5
configure terminal
no router eigrp 20
router eigrp 10
network 10.66.45.0 0.0.0.3
end
```

```lab
sim.advance {"seconds": 20}
sim.advance {"seconds": 1}
```

```verify
R4# show ip eigrp neighbors
  expect: 10.66.45.2
R4# show logging
  expect: EIGRP neighbour debug: neighbour 10.66.45.2 on GigabitEthernet0/0 appeared: up
```

```console R4
undebug all
```

## Reflection

1. Why does the update R1 hears from R2 not contain the LAN behind R3, even though R3 is the
   one with the fault?
2. In Part 1 you saw the same fact three ways: the routing table, the events of the RIP process and
   the routing table's changes. Which would you use to know *when* a route went away?
3. What does a hello tell you that `show ip eigrp neighbors` does not?
4. Which other settings must two EIGRP neighbours agree on, and which of them would show up in a
   hello?
