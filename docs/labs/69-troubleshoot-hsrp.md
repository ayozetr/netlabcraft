# Lab 69 — Troubleshooting HSRP with `debug standby`

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Find an HSRP pair whose routers do not agree on the group number: each router is active for
  its own group, and nobody is standby.
- Read the hellos with `debug standby`: who sent them, which group they are for, and which role
  the sender claims.
- Watch a failover and the way back with the same debugging, and see why a router that
  comes back does not take the active role until it has `preempt`.

## Topology

```text
 R1 (G0/0) ── Fa0/1 ─┐
                     S1 ── Fa0/10 ── PC1
 R2 (G0/0) ── Fa0/2 ─┘
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.69.2/24, HSRP virtual address 192.168.69.1 (priority 110) |
| R2 | Router-2 | G0/0 | 192.168.69.3/24, HSRP virtual address 192.168.69.1 |
| S1 | Switch-24 | | |
| PC1 | PC | Fa0 | 192.168.69.10/24, gateway 192.168.69.1 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "R2", "a_port": "g0/0", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/10"}
```

```console PC1
ipconfig 192.168.69.10 255.255.255.0 192.168.69.1
```

## Part 1 — Two active routers

The technician configured R1 with group 1 and R2, from another note, with group 2. Both use
the same virtual address, so every router believes the configuration is fine.

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.69.2 255.255.255.0
standby 1 ip 192.168.69.1
standby 1 priority 110
no shutdown
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 192.168.69.3 255.255.255.0
standby 2 ip 192.168.69.1
no shutdown
end
debug standby
```

The switch needs its 30 seconds of spanning tree before the ports forward, and the routers
exchange hellos every 3 seconds after that.

```lab
sim.advance {"seconds": 1}
sim.advance {"seconds": 40}
sim.advance {"seconds": 1}
```

```verify
R1# show standby
  expect: Group 1 on GigabitEthernet0/0: active
R2# show standby
  expect: Group 2 on GigabitEthernet0/0: active
```

Both say *active*, and there is no standby anywhere: each router is alone in its group. The
debugging on R2 says why. The hellos R2 hears name the group they are for.

```verify
R2# show logging
  expect: HSRP debug: hello from 192.168.69.2 on GigabitEthernet0/0: the active router of group 1
  expect: HSRP debug: group 2 on GigabitEthernet0/0: active
```

R2 hears R1 announce itself as the active router *of group 1*, and its own group is 2. A
router only listens to its own group, so it ignores the hello. Both routers think the other
does not exist, and both answer for 192.168.69.1: the host's ARP cache decides who gets
its traffic, and it will change at random. The fix is on the router with the wrong number.

```console R2
configure terminal
interface g0/0
no standby 2
standby 1 ip 192.168.69.1
end
```

```lab
sim.advance {"seconds": 15}
sim.advance {"seconds": 1}
```

```verify
R1# show standby
  expect: Group 1 on GigabitEthernet0/0: active (priority 110)
R2# show standby
  expect: Group 1 on GigabitEthernet0/0: standby (priority 100)
R2# show logging
  expect: group 1 on GigabitEthernet0/0 appeared
```

## Part 2 — A failover, and the way back

R1 loses its LAN side. R2 must take over within the hold time (10 seconds by default).

```console R1
configure terminal
interface g0/0
shutdown
end
```

```lab
sim.advance {"seconds": 15}
sim.advance {"seconds": 1}
```

```verify
R2# show standby
  expect: Group 1 on GigabitEthernet0/0: active (priority 100)
R2# show logging
  expect: group 1 on GigabitEthernet0/0 changed: standby, priority 100
  expect: -> active, priority 100
```

The debugging prints the change in one line: what the group was, and what it became. Now R1
comes back, with the better priority and no `preempt`:

```console R1
configure terminal
interface g0/0
no shutdown
end
```

```lab
sim.advance {"seconds": 40}
sim.advance {"seconds": 1}
```

```verify
R1# show standby
  expect: Group 1 on GigabitEthernet0/0: standby (priority 110)
R2# show standby
  expect: Group 1 on GigabitEthernet0/0: active (priority 100)
```

R1 has the higher priority and stays standby: a router that is already active keeps the role
until something forces a change. Whether that is what you want is a design decision; with
`preempt` the better router takes the role back.

```console R1
configure terminal
interface g0/0
standby 1 preempt
end
```

```lab
sim.advance {"seconds": 15}
sim.advance {"seconds": 1}
```

```verify
R1# show standby
  expect: Group 1 on GigabitEthernet0/0: active (priority 110, preempt)
R2# show standby
  expect: Group 1 on GigabitEthernet0/0: standby
R2# show logging
  expect: -> standby, priority 100
```

```console R2
undebug all
```

## Reflection

1. With the wrong group number, why did `show standby` on each router look healthy?
2. What would PC1 notice with two routers answering for 192.168.69.1?
3. In Part 2, why did R1 stay standby after it came back, and what is the price of `preempt`
   when the interface flaps?
4. What does a hello tell you that `show standby` on the same router does not?
