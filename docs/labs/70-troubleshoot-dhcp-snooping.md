# Lab 70 — Troubleshooting: DHCP snooping that blocks the real server

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 30 minutes

## Objectives

- Turn on DHCP snooping on a switch whose DHCP server sits behind one port, forget to trust
  that port, and see what the users notice: nobody new gets an address, though nothing looks
  broken.
- Find the cause from `show ip dhcp snooping` (every port untrusted, a counter of dropped
  messages) and from the event list, which names the port the server's offer arrived on.
- Trust the uplink and check that new clients are served and that their leases are recorded in
  the snooping binding table.

## Topology

```text
 R1 (G0/0) ── Gi0/1 ── S1 ──┬── Fa0/1 ── PC1
   DHCP server              └── Fa0/2 ── PC2
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.70.1/24, DHCP pool (.1 to .10 kept back) |
| S1 | Switch-24 | | |
| PC1, PC2 | PC | Fa0 | from DHCP |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.70.1 255.255.255.0
no shutdown
exit
ip dhcp excluded-address 192.168.70.1 192.168.70.10
ip dhcp pool LAN
network 192.168.70.0 255.255.255.0
default-router 192.168.70.1
end
```

## Part 1 — Before snooping

The switch is still spanning tree's 30 seconds away from forwarding on its new ports.

```lab
sim.advance {"seconds": 35}
```

```console PC1
ipconfig /renew
```

```verify
PC1> ipconfig
  expect: 192.168.70.11
ping PC1 192.168.70.1 => success
```

## Part 2 — Snooping, switched on from a checklist

The checklist said to turn snooping on for VLAN 1 and to turn the option-82 insertion off. It
did not say which ports to trust. The server is behind Gi0/1; every port is untrusted.

```console S1
enable
configure terminal
ip dhcp snooping
ip dhcp snooping vlan 1
no ip dhcp snooping information option
end
```

PC1 keeps the address it has until the lease ends. A new client is the one that notices:

```console PC2
ipconfig /renew
```

```verify
ping PC2 192.168.70.1 => failure
S1# show ip dhcp snooping
  expect: DHCP snooping:            on
  expect: (every port is untrusted, with no rate limit)
  reject: DHCP messages dropped:    0
S1# show ip dhcp snooping binding
  expect: 0 bindings
events: untrusted port Gi0/1
```

Two things say what happened: the counter of dropped messages grows, and the event list shows
the server's offer being thrown away on the port it came in on, because only trusted ports may
answer DHCP clients. The fault is nothing the router can see: R1 sent its offer and the
switch dropped it.

```verify
R1# show ip dhcp pool
  expect: LAN
```

## Part 3 — Trust the uplink

```console S1
configure terminal
interface g0/1
ip dhcp snooping trust
end
```

```console PC2
ipconfig /renew
```

```verify
S1# show ip dhcp snooping
  expect: Gi0/1
  expect: yes
S1# show ip dhcp snooping binding
  expect: 1 binding
  expect: Fa0/2
ping PC2 192.168.70.1 => success
ping PC1 192.168.70.1 => success
```

The binding table now lists PC2 with the port it was seen on and the lease it was given:
Dynamic ARP Inspection and IP Source Guard use that table, which is one reason to get snooping
right before turning those on.

## Reflection

1. Why did PC1 not notice when snooping was switched on, and PC2 did?
2. Which port must be trusted: the one the clients are on, or the one the server is behind? What
   if there are two switches in a row?
3. What does the dropped-message counter tell you that the router's own log would not?
4. Why does a rogue DHCP server not win once the uplink is the only trusted port?
