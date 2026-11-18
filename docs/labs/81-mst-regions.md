# Lab 81 — Multiple spanning tree: regions and one tree per group of VLANs

**Course:** CCNA 2 follow-up (Switching, Routing and Wireless Essentials) · **Time:** 60 minutes

## Objectives

- Turn on multiple spanning tree (MST) and describe a region with a name, a revision number and a table
  of which VLANs belong to which instance.
- Give two instances two different roots, and see that a different link is blocked in each, so
  the ring carries traffic on every link.
- Read `show spanning-tree mst configuration` (the digest the switches compare) and
  `show spanning-tree mst N`.
- Cable in a switch from another region and see the link to it marked as the edge of the region.

## Topology

```text
        S1 ─────── S2 ──(Fa0/3)── S4      (S4 is in another region)
          \G0/2   G0/2/
           \       /
            G0/1 G0/2
               S3
```

| Device | Model | Role |
|---|---|---|
| S1 | Switch-24 | root of instance 1 (VLANs 10 and 20) and of the common tree |
| S2 | Switch-24 | root of instance 2 (VLAN 30) |
| S3 | Switch-24 | third switch of the ring |
| S4 | Switch-24 | a switch in region OTHER |

The three ring switches have the same region: name `LAB`, revision 3, instance 1 holds VLANs 10 and 20,
instance 2 holds VLAN 30. VLANs that no instance claims stay in instance 0, the common tree.

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "Switch-24", "name": "S3"}
device.add {"model": "Switch-24", "name": "S4"}
link.add {"a": "S1", "a_port": "g0/1", "b": "S2", "b_port": "g0/1"}
link.add {"a": "S1", "a_port": "g0/2", "b": "S3", "b_port": "g0/1"}
link.add {"a": "S2", "a_port": "g0/2", "b": "S3", "b_port": "g0/2"}
link.add {"a": "S2", "a_port": "fa0/3", "b": "S4", "b_port": "fa0/3"}
```

## Part 1 — The region

Type the same lines on S1, S2 and S3. The region only exists between switches whose name,
revision and table all agree.

```console S1
enable
configure terminal
interface range g0/1 - 2
switchport mode trunk
exit
vlan 10
vlan 20
vlan 30
spanning-tree mode mst
spanning-tree mst configuration
name LAB
revision 3
instance 1 vlan 10,20
instance 2 vlan 30
exit
spanning-tree mst 0 root primary
spanning-tree mst 1 root primary
end
```

```console S2
enable
configure terminal
interface range g0/1 - 2
switchport mode trunk
exit
interface fa0/3
switchport mode trunk
exit
vlan 10
vlan 20
vlan 30
spanning-tree mode mst
spanning-tree mst configuration
name LAB
revision 3
instance 1 vlan 10,20
instance 2 vlan 30
exit
spanning-tree mst 2 root primary
end
```

```console S3
enable
configure terminal
interface range g0/1 - 2
switchport mode trunk
exit
vlan 10
vlan 20
vlan 30
spanning-tree mode mst
spanning-tree mst configuration
name LAB
revision 3
instance 1 vlan 10,20
instance 2 vlan 30
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
S1# show spanning-tree mst configuration
  expect: Region name:  LAB
  expect: Revision:     3
  expect: 1         10,20
  expect: 2         30
```

The digest is a fingerprint of the VLAN table. Two switches that map the VLANs differently
have different digests, and they are in different regions even if the names match.

## Part 2 — One root per instance

```verify
S1# show spanning-tree mst 1
  expect: Root:   this switch
S2# show spanning-tree mst 2
  expect: Root:   this switch
S3# show spanning-tree mst 1
  expect: path cost
```

Look at which port of S3 is blocking in instance 1 and in instance 2:

```console S3
show spanning-tree mst 1
show spanning-tree mst 2
```

In a ring one link must be blocked for each instance. With S1 as root of instance 1 the blocked
end is on the link between S2 and S3, and with S2 as root of instance 2 it is on the link between
S1 and S3. Traffic of VLANs 10 and 20 and traffic of VLAN 30 therefore use different links.

## Part 3 — The edge of the region

S4 runs MST too but in region OTHER. Its link to S2 is the edge of S2's region.

```console S4
enable
configure terminal
interface fa0/3
switchport mode trunk
exit
spanning-tree mode mst
spanning-tree mst configuration
name OTHER
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
S2# show spanning-tree mst 0
  expect: Internal
  expect: Boundary(MST)
```

Ports inside the region are `Internal`; the port to S4 is a `Boundary` port, and for the
instances outside the region it behaves as the common tree says.

## Reflection

1. Why must the revision number be changed when the table changes, and what happens if one switch
   is not updated?
2. What is gained by two instances instead of one tree for every VLAN, and what would two hundred
   instances cost?
3. A VLAN that is not in any `instance` line: which tree does it follow?
4. Why does the link to S4 not take part in instance 1 and instance 2?
