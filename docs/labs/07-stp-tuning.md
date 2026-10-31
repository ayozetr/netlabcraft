# Lab 7 — Spanning tree: choose the root and move the blocked port

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Watch spanning tree break the loop in a triangle of switches, and find the root bridge,
  root ports, designated ports and the blocked port.
- Choose the root bridge and its backup with `root primary` / `root secondary`.
- Move the blocked port by changing a port cost.
- Switch to Rapid PVST+ and protect the access ports with PortFast and BPDU guard.

## Topology

```text
            S1
     Fa0/1 /  \ Fa0/2
          /    \
   Fa0/1 /      \ Fa0/1
       S2 ────── S3
      Fa0/2   Fa0/2
        |        |
      Fa0/10   Fa0/10
       PC1      PC2
```

| Device | Model | Address |
|---|---|---|
| S1, S2, S3 | Switch-24 | — |
| PC1 | PC (on S2 Fa0/10) | 192.168.1.10/24 |
| PC2 | PC (on S3 Fa0/10) | 192.168.1.20/24 |

All switch-to-switch links are trunks; the PCs are in VLAN 1.

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "Switch-24", "name": "S3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "S1", "a_port": "fa0/1", "b": "S2", "b_port": "fa0/1"}
link.add {"a": "S1", "a_port": "fa0/2", "b": "S3", "b_port": "fa0/1"}
link.add {"a": "S2", "a_port": "fa0/2", "b": "S3", "b_port": "fa0/2"}
link.add {"a": "PC1", "b": "S2", "b_port": "fa0/10"}
link.add {"a": "PC2", "b": "S3", "b_port": "fa0/10"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0
```

```console PC2
ipconfig 192.168.1.20 255.255.255.0
```

## Part 1 — Trunks between the switches

Type the same on S1, S2 and S3:

```console S1
enable
configure terminal
interface range fa0/1 - 2
switchport mode trunk
end
```

```console S2
enable
configure terminal
interface range fa0/1 - 2
switchport mode trunk
end
```

```console S3
enable
configure terminal
interface range fa0/1 - 2
switchport mode trunk
end
```

```lab
sim.advance {"seconds": 35}
```

## Part 2 — Choose the root

Left alone, the switch with the lowest MAC address becomes root. Make S1 the root and S2 its
backup: `root primary` lowers S1's priority below every other switch's (24576 here),
`root secondary` sets S2's to 28672.

```console S1
configure terminal
spanning-tree vlan 1 root primary
end
```

```console S2
configure terminal
spanning-tree vlan 1 root secondary
end
```

```lab
sim.advance {"seconds": 35}
```

S1 is the root, so both its ports are designated. S2 and S3 each reach the root directly
through Fa0/1 (their root ports). On the S2–S3 link, S2 has the better bridge ID, so its
Fa0/2 is designated and S3's Fa0/2 is the one that blocks.

```verify
S1# show spanning-tree vlan 1
  expect: This switch is the root
  expect: 24577
S3# show spanning-tree vlan 1
  expect: reached through Fa0/1
  reject: This switch is the root
S3# show spanning-tree vlan 1
  expect: Fa0/1      Root FWD
  expect: Fa0/2      Altn BLK
ping PC1 192.168.1.20 => success
```

## Part 3 — Move the blocked port

Make S3's direct link to the root expensive. Its best path to S1 is now through S2 (cost
19 + 19 = 38 against 100), so Fa0/2 becomes its root port and Fa0/1 blocks instead.

```console S3
configure terminal
interface fa0/1
spanning-tree vlan 1 cost 100
end
```

```lab
sim.advance {"seconds": 35}
```

```verify
S3# show spanning-tree vlan 1
  expect: reached through Fa0/2
  expect: Fa0/1      Altn BLK
ping PC1 192.168.1.20 => success
```

## Part 4 — Rapid PVST+, PortFast and BPDU guard

Rapid PVST+ converges in a second or two instead of 30–50. Access ports to hosts can skip
the listening and learning states with PortFast, and BPDU guard shuts such a port down if a
switch is ever plugged into it.

```console S1
configure terminal
spanning-tree mode rapid-pvst
end
```

```console S2
configure terminal
spanning-tree mode rapid-pvst
interface fa0/10
switchport mode access
spanning-tree portfast
spanning-tree bpduguard enable
end
```

```console S3
configure terminal
spanning-tree mode rapid-pvst
interface fa0/10
switchport mode access
spanning-tree portfast
spanning-tree bpduguard enable
end
```

```lab
sim.advance {"seconds": 5}
```

```verify
S2# show spanning-tree summary
  expect: rapid-pvst
S2# show running-config
  expect: spanning-tree portfast
  expect: spanning-tree bpduguard enable
ping PC1 192.168.1.20 => success
```

## Reflection

1. Why did S3's Fa0/2, and not S2's, block in Part 2?
2. After Part 3, which port on the S2–S3 link is designated?
3. What would happen to S2's Fa0/10 if a switch were plugged into it?
