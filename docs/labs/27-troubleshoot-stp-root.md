# Lab 27 — Troubleshooting: the wrong switch is the spanning-tree root

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 40 minutes

## Objectives

- Read which switch is the root bridge, and which port is blocked, from `show spanning-tree`.
- See why a switch nobody chose wins the election, and what it does to the traffic path.
- Place the root on purpose with `spanning-tree vlan N root primary` and `secondary`, and protect
  the placement with root guard.

## Topology

```text
            DS1 ───────── DS2           (distribution, G0/1 to G0/1)
              \G0/2     G0/2/
               \         /
                G0/1   G0/2
                  ACC1 ── PC1  (access, Fa0/1)
```

| Device | Model | Role |
|---|---|---|
| DS1 | Switch-24 | distribution switch, should be the root |
| DS2 | Switch-24 | distribution switch, should be the backup root |
| ACC1 | Switch-24 | access switch |
| PC1 | PC | on ACC1 Fa0/1, 10.27.0.1/24 |
| PC2 | PC | on DS2 Fa0/1, 10.27.0.2/24 |

ACC1 is added first, so it has the lowest MAC address of the three. All three have the default
priority, and a tie is broken by the lowest MAC address.

```lab
device.add {"model": "Switch-24", "name": "ACC1"}
device.add {"model": "Switch-24", "name": "DS1"}
device.add {"model": "Switch-24", "name": "DS2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "DS1", "a_port": "g0/1", "b": "DS2", "b_port": "g0/1"}
link.add {"a": "DS1", "a_port": "g0/2", "b": "ACC1", "b_port": "g0/1"}
link.add {"a": "DS2", "a_port": "g0/2", "b": "ACC1", "b_port": "g0/2"}
link.add {"a": "PC1", "b": "ACC1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "DS2", "b_port": "fa0/1"}
```

```console PC1
ipconfig 10.27.0.1 255.255.255.0
```

```console PC2
ipconfig 10.27.0.2 255.255.255.0
```

## Part 1 — Out of the box

The three switches are trunked together and left at the defaults. Nobody has chosen a root.

```console DS1
enable
configure terminal
interface range g0/1 - 2
switchport mode trunk
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

```verify
ping PC1 10.27.0.2 => success
ACC1# show spanning-tree vlan 1
  expect: This switch is the root
DS1# show spanning-tree vlan 1
  expect: reached through
DS2# show spanning-tree vlan 1
  expect: reached through
```

## Part 2 — Diagnose

Ask each switch who it thinks the root is, and which of its ports are blocked.

```verify
DS1# show spanning-tree root
  expect: 0200.0001.0000
  expect: Gi0/2
DS2# show spanning-tree root
  expect: 0200.0001.0000
ACC1# show spanning-tree root
  expect: this switch is the root
ACC1# show spanning-tree bridge
  expect: yes
DS2# show spanning-tree blockedports
  expect: VLAN0001   Gi0/1
  expect: 1 blocked port
DS1# show spanning-tree blockedports
  expect: none
```

ACC1 is the root: with equal priorities, the lowest MAC address wins, and ACC1's is the lowest
(0200.0001.0000). Look at what that does. The distribution switches both reach the root through
their link to ACC1, and DS2 blocks its port on the direct link to DS1, so the one link meant to
join the distribution layer carries nothing. The access switch should never be the root:
traffic between the distribution switches goes through it. A root is a core or distribution
switch, chosen by configuration, not by whichever MAC address happens to be lowest.

## Part 3 — Place the root on purpose

DS1 becomes the root and DS2 the backup. `root primary` lowers the priority just enough to win
the election; `root secondary` makes it the second choice.

```console DS1
configure terminal
spanning-tree vlan 1 root primary
end
```

```console DS2
configure terminal
spanning-tree vlan 1 root secondary
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
DS1# show spanning-tree vlan 1
  expect: This switch is the root
  expect: 24577
DS2# show spanning-tree vlan 1
  expect: 28673
  expect: reached through Gi0/1
ACC1# show spanning-tree vlan 1
  expect: 24577
  expect: Root FWD
  expect: Altn BLK
DS1# show spanning-tree summary
  expect: Root bridge of:       VLAN0001
DS1# show spanning-tree bridge
  expect: yes
ACC1# show spanning-tree blockedports
  expect: VLAN0001   Gi0/2
DS2# show spanning-tree blockedports
  expect: none
ping PC1 10.27.0.2 => success
```

DS1 is the root, DS2 reaches it over the direct link, and the blocked port has moved to the
access switch, where it belongs: ACC1 forwards towards DS1 and blocks its other uplink.

## Part 4 — A rogue switch

A technician connects another switch to ACC1's spare port, and its priority is set to 0 "so it
wins". Add it to the lab:

```lab
device.add {"model": "Switch-24", "name": "ROGUE"}
link.add {"a": "ACC1", "a_port": "fa0/2", "b": "ROGUE", "b_port": "fa0/1"}
```

```console ROGUE
enable
configure terminal
spanning-tree vlan 1 priority 0
interface fa0/1
switchport mode trunk
end
```

```console ACC1
configure terminal
interface fa0/2
switchport mode trunk
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
ROGUE# show spanning-tree vlan 1
  expect: This switch is the root
DS1# show spanning-tree vlan 1
  reject: This switch is the root
```

The rogue switch took the root and the whole tree is rearranged around it. Root guard stops a
port from accepting a better root than the one you chose. Set it on ACC1's port to the rogue:

```console ACC1
configure terminal
interface fa0/2
spanning-tree guard root
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
DS1# show spanning-tree vlan 1
  expect: This switch is the root
ACC1# show spanning-tree inconsistentports
  expect: Fa0/2
  expect: Root
ping PC1 10.27.0.2 => success
```

## Reflection

1. Why did ACC1 win the first election, and why is that a risk in a real network?
2. What is the difference between `root primary` and setting the priority to a number yourself?
3. Where should root guard be set: towards the core or towards the edge? Why?
