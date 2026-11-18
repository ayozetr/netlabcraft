# Lab 67 — Reading `debug` output: who is the spanning-tree root, and when it changed

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Read `debug spanning-tree bpdu` (what a switch hears), `debug spanning-tree root` (the root
  of each VLAN, and every change of it) and `debug spanning-tree events` (each port's role and
  state, and every change of them).
- Catch an unplanned root: a switch brought from another lab, with a low bridge priority, takes
  over the whole network the moment it is cabled in.
- Protect the intended root with root guard, and watch the debugging say the old root is back.

## Topology

```text
 DS1 ───────── DS2 ──── (Fa0/10) ──── LAB1     (LAB1 joins in Part 2)
   \G0/2    G0/2/
    \         /
     G0/1   G0/2
       ACC1
```

| Device | Model | Role |
|---|---|---|
| DS1 | Switch-24 | the intended root (priority 24576, `root primary`) |
| DS2 | Switch-24 | distribution switch |
| ACC1 | Switch-24 | access switch: the debugging runs here |
| LAB1 | Switch-24 | a switch from another lab, with priority 4096 |

All the links are trunks. The debugging runs on ACC1 because that is where the effect of a
new root is easiest to see: its root port moves.

```lab
device.add {"model": "Switch-24", "name": "DS1"}
device.add {"model": "Switch-24", "name": "DS2"}
device.add {"model": "Switch-24", "name": "ACC1"}
device.add {"model": "Switch-24", "name": "LAB1"}
link.add {"a": "DS1", "a_port": "g0/1", "b": "DS2", "b_port": "g0/1"}
link.add {"a": "DS1", "a_port": "g0/2", "b": "ACC1", "b_port": "g0/1"}
link.add {"a": "DS2", "a_port": "g0/2", "b": "ACC1", "b_port": "g0/2"}
```

```console DS1
enable
configure terminal
interface range g0/1 - 2
switchport mode trunk
exit
spanning-tree vlan 1 root primary
end
```

```console DS2
enable
configure terminal
interface range g0/1 - 2
switchport mode trunk
end
```

```console ACC1
enable
configure terminal
interface range g0/1 - 2
switchport mode trunk
end
```

```lab
sim.advance {"seconds": 60}
```

## Part 1 — The normal picture

Turn the three kinds of spanning-tree debugging on at ACC1, give them a moment, and read.

```console ACC1
debug spanning-tree bpdu
debug spanning-tree root
debug spanning-tree events
```

```lab
sim.advance {"seconds": 1}
sim.advance {"seconds": 10}
```

```verify
ACC1# show debugging
  expect: spanning-tree BPDUs
  expect: spanning-tree root changes
  expect: spanning-tree events
ACC1# show logging
  expect: STP BPDU debug: classic BPDU on Gi0/1: root 24577.
  expect: STP root debug: VLAN 1 root: 24577.
  expect: STP event debug: VLAN 1 port Gi0/1: root port (FWD)
  expect: STP event debug: VLAN 1 port Gi0/2: alternate port (BLK)
```

- The BPDU line is a message heard on one port: the root it names (priority 24577, which is
  24576 plus the VLAN number), the cost to it, and the bridge that sent it.
- The root line says where the tree is rooted: the priority and MAC address, and the cost
  from ACC1.
- The event lines give each port's role and state. ACC1's port to DS1 is its root port, and
  the port to DS2 is blocked, because DS2 is a longer way to the same root.

## Part 2 — A switch from another lab

Somebody plugs LAB1 into DS2's Fa0/10. It still has the priority someone gave it for a different
exercise: 4096, much lower than the 24576 of DS1. A lower priority wins the root election.

```console LAB1
enable
configure terminal
interface fa0/1
switchport mode trunk
exit
spanning-tree vlan 1 priority 4096
end
```

```console DS2
configure terminal
interface fa0/10
switchport mode trunk
end
```

```console ACC1
clear logging
```

Now the cable goes in, and the tree has its 60 seconds to settle (the extra second lets the
debugging look once more).

```lab
link.add {"a": "DS2", "a_port": "fa0/10", "b": "LAB1", "b_port": "fa0/1"}
sim.advance {"seconds": 60}
sim.advance {"seconds": 1}
```

```verify
ACC1# show logging
  expect: STP root debug: VLAN 1 root changed: 24577.
  expect: -> 4097.
  expect: STP event debug: VLAN 1 port Gi0/2 changed:
  expect: -> root port (FWD)
  expect: STP BPDU debug: classic BPDU on Gi0/2: root 4097.
ACC1# show spanning-tree vlan 1
  reject: This switch is the root
```

The root changed from 24577 to 4097 with no change on any switch of the production network:
the debugging prints the old root and the new one in the same line, and the port events
show ACC1's traffic switching to the port towards DS2, which is now the way to the root.
Every flow in the network now goes through a laboratory switch.

## Part 3 — Protect the root

Root guard on the port a root must never come from keeps the tree where it belongs: if that port
hears a better root than the current one, it blocks rather than letting the tree move.

```console DS2
configure terminal
interface fa0/10
spanning-tree guard root
end
```

```lab
sim.advance {"seconds": 60}
sim.advance {"seconds": 1}
```

```verify
DS2# show spanning-tree inconsistentports
  expect: Root inconsistent
  expect: Fa0/10
  expect: 1 inconsistent port
ACC1# show logging
  expect: STP root debug: VLAN 1 root changed: 4097.
  expect: -> 24577.
```

The root is back to DS1's 24577. The port towards LAB1 is blocked as root-inconsistent for as
long as LAB1 claims a better root, and recovers by itself when it stops.

```console ACC1
undebug all
```

## Reflection

1. Why does a lower priority beat a higher one, and what breaks a tie between two switches with
   the same priority?
2. The root change was reported on ACC1 even though ACC1's own configuration did not change.
   Which other devices would have noticed it?
3. Which ports should have root guard: the ones facing the core, or the ones facing the
   access layer?
4. What did the BPDU lines tell you that the root line did not?
