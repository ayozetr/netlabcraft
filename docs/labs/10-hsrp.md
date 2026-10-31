# Lab 10 — First-hop redundancy with HSRP

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Give a LAN a virtual default gateway shared by two routers with HSRP version 2.
- Choose the active router with priorities and preemption.
- Track an interface so the routers swap roles when the active one loses its other side.
- Check that hosts keep working when the active router fails.

## Topology

```text
                ┌──G0/0── R1 ──G0/1──┐
 PC1 ── S1 ─────┤                    ├───── S2 ── Server1
                └──G0/0── R2 ──G0/1──┘
```

| Device | Model | Interface | Address | HSRP group, virtual address |
|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.2/24 | 1, 192.168.1.1 (priority 150) |
| R1 | | G0/1 | 10.0.0.2/24 | 2, 10.0.0.1 (priority 150) |
| R2 | Router-2 | G0/0 | 192.168.1.3/24 | 1, 192.168.1.1 (priority 100) |
| R2 | | G0/1 | 10.0.0.3/24 | 2, 10.0.0.1 (priority 100) |
| PC1 | PC | Fa0 | 192.168.1.10/24, gateway 192.168.1.1 | |
| Server1 | Server | G0 | 10.0.0.10/24, gateway 10.0.0.1 | |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "Server", "name": "Server1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "R2", "a_port": "g0/0", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "S2", "b_port": "fa0/1"}
link.add {"a": "R2", "a_port": "g0/1", "b": "S2", "b_port": "fa0/2"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/10"}
link.add {"a": "Server1", "b": "S2", "b_port": "fa0/10"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console Server1
ipconfig 10.0.0.10 255.255.255.0 10.0.0.1
```

## Part 1 — HSRP on both routers

Each LAN gets its own group. The routers' own addresses stay as they are; the hosts only
ever use the virtual address. R1 gets the higher priority, and `preempt` lets a router with a
higher priority take the active role back when it comes up.

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.2 255.255.255.0
standby version 2
standby 1 ip 192.168.1.1
standby 1 priority 150
standby 1 preempt
standby 1 track g0/1 60
no shutdown
interface g0/1
ip address 10.0.0.2 255.255.255.0
standby version 2
standby 2 ip 10.0.0.1
standby 2 priority 150
standby 2 preempt
standby 2 track g0/0 60
no shutdown
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 192.168.1.3 255.255.255.0
standby version 2
standby 1 ip 192.168.1.1
standby 1 preempt
no shutdown
interface g0/1
ip address 10.0.0.3 255.255.255.0
standby version 2
standby 2 ip 10.0.0.1
standby 2 preempt
no shutdown
end
```

The routers exchange hellos (every 3 seconds) and elect the active router; give them time:

```lab
sim.advance {"seconds": 15}
```

## Verification

```verify
R1# show standby brief
  expect: Group 1 on GigabitEthernet0/0: active (priority 150, preempt)
  expect: virtual address 192.168.1.1
  expect: virtual address 10.0.0.1
R2# show standby brief
  expect: Group 1 on GigabitEthernet0/0: standby
  expect: Group 2 on GigabitEthernet0/1: standby
  expect: Active: 192.168.1.2
ping PC1 10.0.0.10 => success
PC1> tracert 10.0.0.10
  expect: 192.168.1.2
```

The trace shows the packets go through R1, the active router of group 1.

## Part 2 — R1 fails

Pull R1's LAN side. R1 can't serve group 1 any more, and its tracking takes 60 off its
group 2 priority (150 → 90), below R2's 100: R2 preempts and becomes active on both LANs.

```console R1
configure terminal
interface g0/0
shutdown
end
```

```lab
sim.advance {"seconds": 15}
```

```verify
R2# show standby brief
  expect: Group 1 on GigabitEthernet0/0: active
  expect: Group 2 on GigabitEthernet0/1: active
ping PC1 10.0.0.10 => success
PC1> tracert 10.0.0.10
  expect: 192.168.1.3
```

Bring R1 back: preemption hands both groups back to it.

```console R1
configure terminal
interface g0/0
no shutdown
end
```

S1's port to R1 went down and up, so it spends 30 seconds listening and learning before it
forwards again (spanning tree): wait for its light to turn green.

```lab
sim.advance {"seconds": 35}
```

```verify
R1# show standby brief
  expect: Group 1 on GigabitEthernet0/0: active
  expect: Group 2 on GigabitEthernet0/1: active
ping PC1 10.0.0.10 => success
```

## Reflection

1. Which MAC address does PC1 have for its gateway in its ARP cache, and why doesn't it
   change when R2 takes over?
2. What would have happened in Part 2 without `standby 2 track g0/0 60`?
3. Why is `preempt` needed for R1 to become active again?
