# Lab 34 — Troubleshooting: an EtherChannel that will not bundle

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Start from a working LACP EtherChannel between two switches, then break it three ways: both
  ends passive, one end with no negotiation, and one member with different settings.
- Tell the three apart from `show etherchannel summary`, and from what spanning tree and a
  `ping` do meanwhile.
- Repair each one, and learn why trunk settings belong on the port-channel and not on its
  members.

## Topology

```text
            Fa0/1 ════ Fa0/1
 PC1 ── S1                   S2 ── PC2
            Fa0/2 ════ Fa0/2
```

| Device | Model | Address |
|---|---|---|
| S1, S2 | Switch-24 | — |
| PC1 | PC (S1 Fa0/10) | 10.34.0.1/24 |
| PC2 | PC (S2 Fa0/10) | 10.34.0.2/24 |

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "S1", "a_port": "fa0/1", "b": "S2", "b_port": "fa0/1"}
link.add {"a": "S1", "a_port": "fa0/2", "b": "S2", "b_port": "fa0/2"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/10"}
link.add {"a": "PC2", "b": "S2", "b_port": "fa0/10"}
```

```console PC1
ipconfig 10.34.0.1 255.255.255.0
```

```console PC2
ipconfig 10.34.0.2 255.255.255.0
```

## Part 1 — The working bundle

S1 starts the LACP negotiation (`active`) and S2 answers (`passive`). The trunk is set on the
port-channel, which copies it to every member.

```console S1
enable
configure terminal
interface range fa0/1 - 2
channel-group 1 mode active
exit
interface port-channel 1
switchport mode trunk
end
```

```console S2
enable
configure terminal
interface range fa0/1 - 2
channel-group 1 mode passive
exit
interface port-channel 1
switchport mode trunk
end
```

```lab
sim.advance {"seconds": 35}
```

```verify
S1# show etherchannel summary
  expect: Port-channel1 (LACP), up, 2 of 2 ports bundled
S2# show etherchannel summary
  expect: Port-channel1 (LACP), up, 2 of 2 ports bundled
S2# show spanning-tree
  reject: BLK
ping PC1 10.34.0.2 => success
```

## Fault 1 — Both ends passive

Someone "tidies" S1's configuration and puts its members in `passive` as well.

```console S1
configure terminal
interface range fa0/1 - 2
no channel-group
channel-group 1 mode passive
end
```

```lab
sim.advance {"seconds": 35}
```

### Diagnose

The hosts still ping, so nothing looks broken. The summary says otherwise:

```verify
ping PC1 10.34.0.2 => success
S1# show etherchannel summary
  expect: Port-channel1 (LACP), down, 0 of 2 ports bundled
  expect: on its own
S2# show etherchannel summary
  expect: down, 0 of 2 ports bundled
S2# show spanning-tree
  expect: Altn BLK
```

A passive port never starts the negotiation and only answers one that is started, so two
passive ends wait for each other for ever. The ports work alone ("on its own"), spanning tree
sees two parallel links and blocks one: the second cable carries nothing.

### Repair

```console S1
configure terminal
interface range fa0/1 - 2
no channel-group
channel-group 1 mode active
end
```

```lab
sim.advance {"seconds": 35}
```

```verify
S1# show etherchannel summary
  expect: Port-channel1 (LACP), up, 2 of 2 ports bundled
S2# show spanning-tree
  reject: BLK
ping PC1 10.34.0.2 => success
```

## Fault 2 — One end with no negotiation

A new technician sets S2's members to `on`, which bundles without any protocol.

```console S2
configure terminal
interface range fa0/1 - 2
no channel-group
channel-group 1 mode on
end
```

```lab
sim.advance {"seconds": 35}
```

### Diagnose

This time the network is cut:

```verify
ping PC1 10.34.0.2 => failure
S2# show etherchannel summary
  expect: Port-channel1 (none), down, 0 of 2 ports bundled
  expect: suspended
S1# show etherchannel summary
  expect: Port-channel1 (LACP), down, 0 of 2 ports bundled
  expect: on its own
```

The two ends do not speak the same protocol: S1 sends LACP and S2 has no protocol, so S2
cannot confirm the bundle. A member in `on` mode that does not agree with its neighbour is
**suspended** and passes nothing, to protect the network from a half-bundle. S1's ports work
alone, but there is nobody to talk to on the other side. The mode column of the summary and
the protocol in the first line (LACP, PAgP or none) point at the mismatch.

### Repair

Put S2 back to LACP (`passive` answers S1's `active`):

```console S2
configure terminal
interface range fa0/1 - 2
no channel-group
channel-group 1 mode passive
end
```

```lab
sim.advance {"seconds": 35}
```

```verify
S1# show etherchannel summary
  expect: Port-channel1 (LACP), up, 2 of 2 ports bundled
S2# show etherchannel summary
  expect: Port-channel1 (LACP), up, 2 of 2 ports bundled
ping PC1 10.34.0.2 => success
```

## Fault 3 — One member with different settings

All members of a bundle must have the same switchport settings. Someone restricts the VLANs
on one cable of S1, directly on the member and not on the port-channel:

```console S1
configure terminal
interface fa0/2
switchport trunk allowed vlan 10
end
```

```lab
sim.advance {"seconds": 35}
```

### Diagnose

```verify
S1# show etherchannel summary
  expect: Port-channel1 (LACP), up, 1 of 2 ports bundled
  expect: suspended
S1# show interfaces fa0/1 switchport
  expect: Trunk allowed VLANs: all
S1# show interfaces fa0/2 switchport
  expect: Trunk allowed VLANs: 10
```

The channel is up with one of two members: Fa0/2 is suspended because its settings differ
from the rest, and the bundle carries on with Fa0/1 alone, so a single failure from here
costs the whole path. Comparing the two members' `show interfaces … switchport` shows the
difference.

### Repair

Take the extra restriction off the member (and, in future, change trunk settings on
`interface port-channel 1`, which applies them to every member at once):

```console S1
configure terminal
interface fa0/2
no switchport trunk allowed vlan
end
```

```lab
sim.advance {"seconds": 35}
```

## Prove the repair

```verify
S1# show etherchannel summary
  expect: Port-channel1 (LACP), up, 2 of 2 ports bundled
  reject: suspended
  reject: on its own
S2# show etherchannel summary
  expect: Port-channel1 (LACP), up, 2 of 2 ports bundled
S2# show spanning-tree
  reject: BLK
S1# show interfaces trunk
  expect: Po1
ping PC1 10.34.0.2 => success
ping PC2 10.34.0.1 => success
```

## Reflection

1. Which fault left the hosts working, and why is that more dangerous than one that cuts them?
2. Which pairs of modes bundle, and which never do (`active`/`active`, `passive`/`passive`,
   `on`/`active`, `desirable`/`auto`)?
3. What does "suspended" protect against, and how is it different from "on its own"?
4. Why is it better to set trunk options on the port-channel than on each member?
