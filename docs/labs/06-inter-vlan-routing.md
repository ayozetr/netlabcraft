# Lab 6 — Inter-VLAN routing: router on a stick and a multilayer switch

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 50 minutes

## Objectives

- Route between two VLANs with one router interface split into 802.1Q subinterfaces.
- Route between two other VLANs on a multilayer switch with switch virtual interfaces.
- Join the two with a routed switch port and static routes, and trace the path.

## Topology

```text
 PC1 (VLAN 10) ─Fa0/1─┐                       ┌─Fa0/1─ PC3 (VLAN 30)
                      S1 ─G0/1══(G0/0) R1 (G0/1)──G0/1 (routed) MLS1
 PC2 (VLAN 20) ─Fa0/2─┘    trunk              └─Fa0/2─ PC4 (VLAN 40)
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0.10 | 10.1.10.1/24 |
| R1 | | G0/0.20 | 10.1.20.1/24 |
| R1 | | G0/1 | 10.1.0.1/30 |
| MLS1 | Switch-ML-24 | G0/1 (routed) | 10.1.0.2/30 |
| MLS1 | | VLAN 30 | 10.1.30.1/24 |
| MLS1 | | VLAN 40 | 10.1.40.1/24 |
| S1 | Switch-24 | — | — |
| PC1 | PC | Fa0 | 10.1.10.10/24, gw 10.1.10.1 |
| PC2 | PC | Fa0 | 10.1.20.10/24, gw 10.1.20.1 |
| PC3 | PC | Fa0 | 10.1.30.10/24, gw 10.1.30.1 |
| PC4 | PC | Fa0 | 10.1.40.10/24, gw 10.1.40.1 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-ML-24", "name": "MLS1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
device.add {"model": "PC", "name": "PC4"}
link.add {"a": "S1", "a_port": "g0/1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/1", "b": "MLS1", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "PC3", "b": "MLS1", "b_port": "fa0/1"}
link.add {"a": "PC4", "b": "MLS1", "b_port": "fa0/2"}
```

```console PC1
ipconfig 10.1.10.10 255.255.255.0 10.1.10.1
```

```console PC2
ipconfig 10.1.20.10 255.255.255.0 10.1.20.1
```

```console PC3
ipconfig 10.1.30.10 255.255.255.0 10.1.30.1
```

```console PC4
ipconfig 10.1.40.10 255.255.255.0 10.1.40.1
```

## Part 1 — Router on a stick

The switch tags each VLAN on its trunk to the router; the router has one subinterface per
VLAN, each with the VLAN it accepts (`encapsulation dot1Q`) and the gateway address for it.

```console S1
enable
configure terminal
vlan 10
name Sales
vlan 20
name Engineering
exit
interface fa0/1
switchport mode access
switchport access vlan 10
interface fa0/2
switchport mode access
switchport access vlan 20
interface g0/1
switchport mode trunk
end
```

```console R1
enable
configure terminal
interface g0/0
no shutdown
interface g0/0.10
description Sales gateway
encapsulation dot1Q 10
ip address 10.1.10.1 255.255.255.0
interface g0/0.20
description Engineering gateway
encapsulation dot1Q 20
ip address 10.1.20.1 255.255.255.0
interface g0/1
ip address 10.1.0.1 255.255.255.252
no shutdown
exit
ip route 10.1.30.0 255.255.255.0 10.1.0.2
ip route 10.1.40.0 255.255.255.0 10.1.0.2
end
```

## Part 2 — A multilayer switch

`ip routing` turns the switch into a router. Each VLAN gets a switch virtual interface as its
gateway, and the uplink to R1 becomes a routed port with `no switchport`.

```console MLS1
enable
configure terminal
ip routing
vlan 30
name Design
vlan 40
name Support
exit
interface fa0/1
switchport mode access
switchport access vlan 30
interface fa0/2
switchport mode access
switchport access vlan 40
exit
interface vlan 30
ip address 10.1.30.1 255.255.255.0
no shutdown
interface vlan 40
ip address 10.1.40.1 255.255.255.0
no shutdown
interface g0/1
no switchport
ip address 10.1.0.2 255.255.255.252
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 10.1.0.1
end
```

Wait for the new switch ports to start forwarding:

```lab
sim.advance {"seconds": 35}
```

## Verification

```verify
R1# show ip interface brief
  expect: GigabitEthernet0/0.10
  expect: 10.1.20.1
R1# show ip route
  expect: GigabitEthernet0/0.10
  expect: 10.1.30.0/24         via 10.1.0.2
MLS1# show ip route
  expect: 10.1.30.0/24
  expect: S*
MLS1# show ip interface brief
  expect: Vlan30
  expect: 10.1.0.2
S1# show interfaces trunk
  expect: Gi0/1
ping PC1 10.1.20.10 => success
ping PC3 10.1.40.10 => success
ping PC1 10.1.40.10 => success
ping PC4 10.1.20.10 => success
PC1> tracert 10.1.30.10
  expect: 10.1.10.1
  expect: 10.1.0.2
  expect: 10.1.30.10
```

## Reflection

1. Why must the router's physical interface G0/0 be turned on but have no address?
2. Which device routes a packet from PC3 to PC4? From PC1 to PC2?
3. What would change if MLS1's G0/1 were left as a switch port?
