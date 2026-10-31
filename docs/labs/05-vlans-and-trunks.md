# Lab 5 — VLANs and 802.1Q trunks

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Create VLANs with names on two switches and put access ports in them.
- Join the switches with a static 802.1Q trunk, with an unused native VLAN, only the needed
  VLANs allowed and DTP negotiation turned off.
- Give each switch a management address in its own VLAN.
- Check that hosts in the same VLAN reach each other across the trunk, and hosts in
  different VLANs don't (there is no router yet).

## Topology

```text
 PC1 (VLAN 10) ─Fa0/6─┐                    ┌─Fa0/6─ PC3 (VLAN 10)
                      S1 ──G0/1 ═══ G0/1── S2
 PC2 (VLAN 20) ─Fa0/11┘    trunk           └─Fa0/11 PC4 (VLAN 20)
```

| VLAN | Name | Subnet |
|---|---|---|
| 10 | Faculty | 192.168.10.0/24 |
| 20 | Students | 192.168.20.0/24 |
| 99 | Management | 192.168.99.0/24 |
| 1000 | Native | — (no hosts) |

| Device | Model | Interface | Address |
|---|---|---|---|
| S1 | Switch-24 | VLAN 99 | 192.168.99.11/24 |
| S2 | Switch-24 | VLAN 99 | 192.168.99.12/24 |
| PC1 | PC | S1 Fa0/6 | 192.168.10.1/24 |
| PC2 | PC | S1 Fa0/11 | 192.168.20.1/24 |
| PC3 | PC | S2 Fa0/6 | 192.168.10.2/24 |
| PC4 | PC | S2 Fa0/11 | 192.168.20.2/24 |

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
device.add {"model": "PC", "name": "PC4"}
link.add {"a": "S1", "a_port": "g0/1", "b": "S2", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/6"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/11"}
link.add {"a": "PC3", "b": "S2", "b_port": "fa0/6"}
link.add {"a": "PC4", "b": "S2", "b_port": "fa0/11"}
```

```console PC1
ipconfig 192.168.10.1 255.255.255.0
```

```console PC2
ipconfig 192.168.20.1 255.255.255.0
```

```console PC3
ipconfig 192.168.10.2 255.255.255.0
```

```console PC4
ipconfig 192.168.20.2 255.255.255.0
```

## Part 1 — VLANs and access ports

Do the same on both switches (S2 shown second, with its own management address).

```console S1
enable
configure terminal
hostname S1
vlan 10
name Faculty
vlan 20
name Students
vlan 99
name Management
vlan 1000
name Native
exit
interface range fa0/1 - 10
switchport mode access
switchport access vlan 10
interface range fa0/11 - 20
switchport mode access
switchport access vlan 20
exit
interface vlan 99
ip address 192.168.99.11 255.255.255.0
no shutdown
end
```

```console S2
enable
configure terminal
hostname S2
vlan 10
name Faculty
vlan 20
name Students
vlan 99
name Management
vlan 1000
name Native
exit
interface range fa0/1 - 10
switchport mode access
switchport access vlan 10
interface range fa0/11 - 20
switchport mode access
switchport access vlan 20
exit
interface vlan 99
ip address 192.168.99.12 255.255.255.0
no shutdown
end
```

Before the trunk is set up, the uplinks are dynamic ports in VLAN 1: PC1 and PC3 are in
VLAN 10 on different switches and can't reach each other yet.

```verify
S1# show vlan brief
  expect: Faculty
  expect: Students
  expect: Management
  expect: Fa0/6
ping PC1 192.168.10.2 => failure
```

## Part 2 — The trunk

```console S1
configure terminal
interface g0/1
switchport mode trunk
switchport trunk native vlan 1000
switchport trunk allowed vlan 10,20,99,1000
switchport nonegotiate
end
```

```console S2
configure terminal
interface g0/1
switchport mode trunk
switchport trunk native vlan 1000
switchport trunk allowed vlan 10,20,99,1000
switchport nonegotiate
end
```

New trunk ports go through spanning tree's listening and learning states before they
forward; in Realtime mode wait for the link lights to turn green (about 30 seconds), or fast
forward:

```lab
sim.advance {"seconds": 35}
```

## Verification

```verify
S1# show interfaces trunk
  expect: Gi0/1
  expect: trunking
  expect: 10,20,99,1000
S1# show interfaces g0/1 switchport
  expect: trunk
  expect: no negotiation
  expect: native VLAN: 1000
ping PC1 192.168.10.2 => success
ping PC2 192.168.20.2 => success
ping PC1 192.168.20.2 => failure
```

The management interfaces are in VLAN 99, which is carried by the trunk, but no access port
is in VLAN 99 yet:

```verify
ping S1 192.168.99.12 => success
```

## Reflection

1. Why does PC1 still fail to reach PC4, even though the trunk carries both VLANs?
2. What happens to untagged frames that arrive on the trunk, and why is it good practice to
   make the native VLAN one with no hosts?
3. What does `switchport nonegotiate` turn off, and why would you want that on a trunk to a
   switch you manage?
