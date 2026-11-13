# Lab 21 — Troubleshooting: VLAN and trunk faults

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 40 minutes

## Objectives

- Start from a working two-switch, two-VLAN network and break it with two faults you did not write.
- Follow the symptoms to their cause with `show vlan brief`, `show interfaces trunk`,
  `show interfaces X switchport`, `show mac address-table` and `ping`.
- Repair each fault and prove the repair.

## Topology

```text
 PC1 (VLAN 10) ─Fa0/1─┐                   ┌─Fa0/1─ PC2 (VLAN 10)
                      S1 ──G0/1 ═══ G0/1── S2
 PC3 (VLAN 20) ─Fa0/2─┘    trunk          └─Fa0/2─ PC4 (VLAN 20)
```

| Device | Model | Port | Address |
|---|---|---|---|
| PC1 | PC | S1 Fa0/1 | 10.21.10.1/24 |
| PC2 | PC | S2 Fa0/1 | 10.21.10.2/24 |
| PC3 | PC | S1 Fa0/2 | 10.21.20.1/24 |
| PC4 | PC | S2 Fa0/2 | 10.21.20.2/24 |

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
device.add {"model": "PC", "name": "PC4"}
link.add {"a": "S1", "a_port": "g0/1", "b": "S2", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC3", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "PC2", "b": "S2", "b_port": "fa0/1"}
link.add {"a": "PC4", "b": "S2", "b_port": "fa0/2"}
```

```console PC1
ipconfig 10.21.10.1 255.255.255.0
```

```console PC2
ipconfig 10.21.10.2 255.255.255.0
```

```console PC3
ipconfig 10.21.20.1 255.255.255.0
```

```console PC4
ipconfig 10.21.20.2 255.255.255.0
```

## Part 1 — The working network

Both switches get the same configuration. The trunk allows only the two VLANs in use.

```console S1
enable
configure terminal
vlan 10
name Sales
vlan 20
name Labs
exit
interface fa0/1
switchport mode access
switchport access vlan 10
interface fa0/2
switchport mode access
switchport access vlan 20
interface g0/1
switchport mode trunk
switchport trunk allowed vlan 10,20
end
```

```console S2
enable
configure terminal
vlan 10
name Sales
vlan 20
name Labs
exit
interface fa0/1
switchport mode access
switchport access vlan 10
interface fa0/2
switchport mode access
switchport access vlan 20
interface g0/1
switchport mode trunk
switchport trunk allowed vlan 10,20
end
```

```lab
sim.advance {"seconds": 35}
```

```verify
ping PC1 10.21.10.2 => success
ping PC3 10.21.20.2 => success
ping PC1 10.21.20.1 => failure
```

## Part 2 — The faults

A colleague "tidied up" overnight. Type the lines below as they are; do not read them as a
model of good practice.

```console S2
configure terminal
interface fa0/1
switchport access vlan 20
exit
interface g0/1
switchport trunk allowed vlan 10
end
```

## Part 3 — Diagnose

Report the symptoms first.

```verify
ping PC1 10.21.10.2 => failure
ping PC3 10.21.20.2 => failure
```

Both pairs are broken, but for different reasons. Start at the edge. Which VLAN is PC2's port in?

```verify
S2# show vlan brief
  expect: Labs
  expect: Fa0/1
S2# show interfaces fa0/1 switchport
  expect: Access VLAN: 20 (Labs)
```

PC2's port is in VLAN 20, but PC2 uses the VLAN 10 subnet. Make PC2 speak, and the switch
shows where it hears it:

```verify
ping PC2 10.21.10.1 => failure
S2# show mac address-table
  expect: 20
  expect: Fa0/1
```

PC2's frames are learned in VLAN 20, never in VLAN 10. That explains the first pair. For the
second, PC3 and PC4 are both in VLAN 20, so look at the link between the switches.

```verify
S1# show interfaces trunk
  expect: 10,20
S2# show interfaces g0/1 trunk
  expect: Gi0/1
  reject: 10,20
S2# show interfaces switchport
  expect: Access VLAN: 20 (Labs)
S2# show running-config interface g0/1
  expect: switchport trunk allowed vlan 10
```

S2's side allows only VLAN 10. A VLAN that one end does not allow is dropped at that end, so
VLAN 20 never crosses.

## Part 4 — Repair

```console S2
configure terminal
interface fa0/1
switchport access vlan 10
exit
interface g0/1
switchport trunk allowed vlan add 20
end
```

The access port goes through spanning tree's listening and learning states again, so wait
before testing:

```lab
sim.advance {"seconds": 35}
```

```verify
S2# show interfaces trunk
  expect: 10,20
S2# show interfaces fa0/1 switchport
  expect: Access VLAN: 10 (Sales)
ping PC1 10.21.10.2 => success
ping PC3 10.21.20.2 => success
ping PC1 10.21.20.1 => failure
```

## Reflection

1. The two faults gave the same symptom (a failed ping). What did each `show` command rule in or out?
2. Why did `show mac address-table` help with the first fault but not with the second?
3. What would you check next if both ends of the trunk allowed VLAN 20 and the ping still failed?
