# Lab 30 — Inspecting spanning tree: roles, states and the blocked port per VLAN

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Read a ring of three switches with `show spanning-tree brief`, `show spanning-tree detail`
  and `show spanning-tree interface`, and tell which port is blocked in each VLAN before
  looking.
- See that spanning tree runs per VLAN: the same port is forwarding in one VLAN and blocked in
  another when the VLANs have different roots.
- Predict, then check, what a change of path cost does to the blocked port, and put the cost
  back.

## Topology

```text
                SW1
            G0/1   G0/2
             /       \
         G0/1         G0/2
        SW2 G0/2──G0/1 SW3
```

SW1 G0/1 is cabled to SW2 G0/1, SW2 G0/2 to SW3 G0/1, and SW3 G0/2 to SW1 G0/2. All three links
are Gigabit Ethernet trunks, so each has a path cost of 4.

| Device | Model | Role |
|---|---|---|
| SW1 | Switch-24 | root of VLAN 10 |
| SW2 | Switch-24 | root of VLAN 20 |
| SW3 | Switch-24 | has no preference (the switch added last has the highest MAC) |
| PC1 | PC | SW1 Fa0/1, VLAN 10, 10.30.0.1/24 |
| PC3 | PC | SW3 Fa0/1, VLAN 10, 10.30.0.3/24 |

```lab
device.add {"model": "Switch-24", "name": "SW1"}
device.add {"model": "Switch-24", "name": "SW2"}
device.add {"model": "Switch-24", "name": "SW3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "SW1", "a_port": "g0/1", "b": "SW2", "b_port": "g0/1"}
link.add {"a": "SW2", "a_port": "g0/2", "b": "SW3", "b_port": "g0/1"}
link.add {"a": "SW3", "a_port": "g0/2", "b": "SW1", "b_port": "g0/2"}
link.add {"a": "PC1", "b": "SW1", "b_port": "fa0/1"}
link.add {"a": "PC3", "b": "SW3", "b_port": "fa0/1"}
```

```console PC1
ipconfig 10.30.0.1 255.255.255.0
```

```console PC3
ipconfig 10.30.0.3 255.255.255.0
```

## Part 1 — Build the ring

Three VLANs are in play: the default VLAN 1, VLAN 10 (the PCs) and VLAN 20 (no hosts; it only
carries a spanning tree). SW1 is made the root of VLAN 10 and SW2 the root of VLAN 20, so the
ring is cut in two different places.

```console SW1
enable
configure terminal
vlan 10
vlan 20
interface range g0/1 - 2
switchport mode trunk
exit
interface fa0/1
switchport mode access
switchport access vlan 10
spanning-tree portfast
exit
spanning-tree vlan 10 root primary
end
```

```console SW2
enable
configure terminal
vlan 10
vlan 20
interface range g0/1 - 2
switchport mode trunk
exit
spanning-tree vlan 20 root primary
end
```

```console SW3
enable
configure terminal
vlan 10
vlan 20
interface range g0/1 - 2
switchport mode trunk
exit
interface fa0/1
switchport mode access
switchport access vlan 10
spanning-tree portfast
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
ping PC1 10.30.0.3 => success
```

## Part 2 — One line per VLAN

`show spanning-tree brief` has a row for each VLAN: who the root is, what it costs to get
there, through which port, and how many ports forward, learn or block.

```verify
SW1# show spanning-tree brief
  expect: VLAN0010
  expect: (this switch)
SW2# show spanning-tree brief
  expect: VLAN0020
  expect: (this switch)
SW3# show spanning-tree brief
  expect: VLAN0010
  expect: VLAN0020
  reject: (this switch)
```

On SW3 no row says "this switch": SW3 is nobody's root. Each of its rows names a root port, and
the rows do not all name the same one: VLANs 1 and 10 go to SW1 through Gi0/2, VLAN 20 goes to
SW2 through Gi0/1. (VLAN 1 has no root of its own choosing: the lowest bridge ID wins, and SW1 was
added first, so it has the lowest MAC address.) Work out which of SW3's ports blocks in each VLAN
before checking:

```verify
SW3# show spanning-tree brief
  expect: Gi0/1
  expect: Gi0/2
SW3# show spanning-tree blockedports
  expect: VLAN0010
  expect: VLAN0020
  expect: 3 blocked ports
```

SW3 has two ports in the ring and each VLAN blocks one of them, so the same port cannot be
right in every VLAN. For VLAN 10 the root is SW1, one link away on G0/2 (cost 4) or two links away through SW2 on
G0/1 (cost 8): G0/2 is the root port and G0/1 blocks. For VLAN 20 it is the other way round.

## Part 3 — One port, every VLAN

`show spanning-tree interface` takes one port and gives its role and state in each VLAN, with
the protection settings (edge port, BPDU guard, root guard, loop guard, BPDU filter) once at
the top.

```verify
SW3# show spanning-tree interface g0/1
  expect: GigabitEthernet0/1
  expect: VLAN0010
  expect: alternate
  expect: VLAN0020
  expect: root
SW3# show spanning-tree interface g0/2
  expect: VLAN0010
  expect: root
  expect: alternate
SW3# show spanning-tree interface fa0/1
  expect: edge port yes
  expect: edge
```

Fa0/1 is the PC's port, set to PortFast: it is an edge port, so it forwards at once and takes no
part in the ring.

The same port, G0/1, is blocking in VLAN 10 (an alternate port) and forwarding in VLAN 20 (its
root port). A trunk carries both VLANs; spanning tree decides per VLAN.

`show spanning-tree detail` goes through the same ports with more: the port ID, the path cost
and where it came from, the link type and the protection settings.

```verify
SW3# show spanning-tree detail
  expect: VLAN0010
  expect: VLAN0020
  expect: Path cost 4 (from speed)
  expect: blocking
SW3# show spanning-tree vlan 10 detail
  expect: This switch
```

## Part 4 — Move the blocked port

Make SW3's direct link to SW1 expensive for VLAN 10 only. Before typing it, say what you expect:
SW3's best path to the root of VLAN 10 is now through SW2 (cost 8) rather than the direct link
(cost 20).

```console SW3
configure terminal
interface g0/2
spanning-tree vlan 10 cost 20
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
SW3# show spanning-tree interface g0/2
  expect: VLAN0010
  expect: alternate
  expect: VLAN0020
SW3# show spanning-tree interface g0/1
  expect: VLAN0010
  expect: root
SW3# show spanning-tree brief
  expect: VLAN0010
  expect: VLAN0020
ping PC1 10.30.0.3 => success
```

The roles for VLAN 10 on SW3 swapped, and VLAN 20 did not change: the cost was set for VLAN 10.
`show spanning-tree brief` still has a row per VLAN; the root port in VLAN 10's row moved.

Put the cost back and check that the tree returns to what it was:

```console SW3
configure terminal
interface g0/2
no spanning-tree vlan 10 cost
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
SW3# show spanning-tree interface g0/1
  expect: VLAN0010
  expect: alternate
SW3# show spanning-tree blockedports
  expect: 3 blocked ports
ping PC1 10.30.0.3 => success
```

## Reflection

1. SW3 blocks one port in each of three VLANs, but only two of its ports are in the ring.
   Which VLANs share a blocked port, and why? Where does VLAN 20 send its traffic compared
   with VLAN 10, and why is that more useful than one tree for the whole ring?
2. Which column of `show spanning-tree brief` would you read first to find out which VLANs a
   switch is the root of? Which one tells you how far a switch is from its root?
3. A port is "alternate" in one VLAN and "root" in another. Which commands tell you that
   without looking at the cabling?
4. You raised a cost on SW3's G0/2 only. Why did the roles on SW1 and SW2 not change?
