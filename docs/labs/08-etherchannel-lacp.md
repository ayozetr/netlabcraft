# Lab 8 — EtherChannel with LACP

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 30 minutes

## Objectives

- Bundle two links between switches into one logical link with LACP.
- Make the bundle a trunk.
- Check that spanning tree sees one link (nothing blocks) and that losing a cable doesn't
  break the path.

## Topology

```text
            Fa0/1 ════ Fa0/1
 PC1 ── S1                   S2 ── PC2
            Fa0/2 ════ Fa0/2
```

| Device | Model | Address |
|---|---|---|
| S1, S2 | Switch-24 | — |
| PC1 | PC (S1 Fa0/10) | 10.0.0.1/24 |
| PC2 | PC (S2 Fa0/10) | 10.0.0.2/24 |

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
ipconfig 10.0.0.1 255.255.255.0
```

```console PC2
ipconfig 10.0.0.2 255.255.255.0
```

## Part 1 — Before the bundle

With two parallel links, spanning tree blocks one of them on the switch that isn't root:

```lab
sim.advance {"seconds": 35}
```

```verify
S1# show spanning-tree
  expect: This switch is the root
S2# show spanning-tree
  expect: Fa0/1      Root FWD
  expect: Fa0/2      Altn BLK
ping PC1 10.0.0.2 => success
```

S1 has the lower MAC address, so it is the root here and the blocked port is on S2 (its
Fa0/2, the port with the higher port ID).

## Part 2 — LACP

Both sides must agree: `active` starts the negotiation, `passive` only answers. The members
are configured together with `interface range`, and the bundle appears as interface
Port-channel 1, where the trunk is set.

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

## Verification

```verify
S1# show etherchannel summary
  expect: Port-channel1 (LACP), up, 2 of 2 ports bundled
  expect: Fa0/1
  expect: Fa0/2
S1# show interfaces trunk
  expect: Po1
  reject: Fa0/1
S2# show spanning-tree
  expect: Po1
  reject: BLK
ping PC1 10.0.0.2 => success
```

Unplug one member (shut it down) and the channel carries on with the other:

```console S1
configure terminal
interface fa0/1
shutdown
end
```

```verify
ping PC1 10.0.0.2 => success
S1# show etherchannel summary
  expect: 1 of 2 ports bundled
```

## Reflection

1. Why does spanning tree no longer block a port once the channel is up?
2. What would happen with `mode passive` on both switches?
3. Why are trunk settings made on the port-channel and not on its members?
