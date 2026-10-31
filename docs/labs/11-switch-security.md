# Lab 11 — Switch security: port security, DHCP snooping and DAI

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 50 minutes

## Objectives

- Harden access ports: access mode, no trunk negotiation, unused ports shut down.
- Limit a port to one MAC address and watch a second device trip the violation.
- Stop a rogue DHCP server with DHCP snooping.
- Stop hosts that aren't in the snooping table from using ARP, with Dynamic ARP Inspection.

## Topology

```text
                         ┌─Fa0/1── PC1 (DHCP)
 R1 (G0/0) ──G0/1── S1 ──┼─Fa0/2── Rogue (server handing out bad addresses)
   DHCP server           ├─Fa0/3── Hub1 ── PC2, PC3
                         └─Fa0/4── PC4 (static address)
```

| Device | Model | Address |
|---|---|---|
| R1 | Router-2 | G0/0 192.168.1.1/24, DHCP pool 192.168.1.0/24 (.1–.10 kept back) |
| S1 | Switch-24 | — |
| PC1 | PC | DHCP |
| Rogue | Server | 10.66.66.1/24, its own DHCP pool 10.66.66.100+ |
| PC2, PC3 | PC (through Hub1, a Hub-8) | 192.168.1.52/24, 192.168.1.53/24 |
| PC4 | PC | 192.168.1.54/24 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Hub-8", "name": "Hub1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
device.add {"model": "PC", "name": "PC4"}
device.add {"model": "Server", "name": "Rogue"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "Rogue", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "Hub1", "b": "S1", "b_port": "fa0/3"}
link.add {"a": "PC2", "b": "Hub1"}
link.add {"a": "PC3", "b": "Hub1"}
link.add {"a": "PC4", "b": "S1", "b_port": "fa0/4"}
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
exit
ip dhcp excluded-address 192.168.1.1 192.168.1.10
ip dhcp pool LAN
network 192.168.1.0 255.255.255.0
default-router 192.168.1.1
end
```

The rogue server is set up in its Services tab (here with the equivalent command):

```console Rogue
ipconfig 10.66.66.1 255.255.255.0
```

```lab
server.services {"device": "Rogue", "services": {"dhcp": {"on": true, "pools": [{"name": "evil", "gateway": "10.66.66.1", "dns": "10.66.66.1", "start": "10.66.66.100", "mask": "255.255.255.0", "max_users": 50}]}}}
```

```console PC2
ipconfig 192.168.1.52 255.255.255.0 192.168.1.1
```

```console PC3
ipconfig 192.168.1.53 255.255.255.0 192.168.1.1
```

```console PC4
ipconfig 192.168.1.54 255.255.255.0 192.168.1.1
```

## Part 1 — Harden the access ports

```console S1
enable
configure terminal
interface range fa0/1 - 24
switchport mode access
switchport nonegotiate
interface range fa0/5 - 24
shutdown
end
```

## Part 2 — Port security

Fa0/3 may carry one MAC address, learned from the first frame and kept in the
configuration (`sticky`). A second address shuts the port down.

```console S1
configure terminal
interface fa0/3
switchport port-security
switchport port-security maximum 1
switchport port-security mac-address sticky
switchport port-security violation shutdown
end
```

```verify
ping PC2 192.168.1.1 => success
S1# show port-security interface fa0/3
  expect: secure-up
```

Now PC3, behind the same hub, sends a frame:

```verify
ping PC3 192.168.1.1 => failure
S1# show port-security interface fa0/3
  expect: err-disabled
S1# show interfaces status
  expect: err-disabled
ping PC2 192.168.1.1 => failure
S1# show running-config
  expect: switchport port-security mac-address sticky
```

The whole port is down, PC2 included. To bring it back, remove the cause and type
`shutdown` and `no shutdown` on the port.

## Part 3 — DHCP snooping

With snooping on, DHCP server messages (Offer, Ack) are only accepted on trusted ports.
Only the uplink to R1 is trusted, so the rogue server's offers are dropped at Fa0/2.

```console S1
configure terminal
ip dhcp snooping
ip dhcp snooping vlan 1
no ip dhcp snooping information option
interface g0/1
ip dhcp snooping trust
interface fa0/1
ip dhcp snooping limit rate 10
end
```

```console PC1
ipconfig /renew
```

```verify
PC1> ipconfig
  expect: 192.168.1.11
  reject: 10.66.66.
S1# show ip dhcp snooping
  expect: 1
S1# show ip dhcp snooping binding
  expect: 192.168.1.11
  expect: Fa0/1
ping PC1 192.168.1.1 => success
```

## Part 4 — Dynamic ARP Inspection

DAI checks every ARP message on untrusted ports against the snooping table. PC1 got its
address by DHCP, so it is in the table; PC4 was set by hand, so its ARP requests are
dropped and it can't reach anything until it, too, uses DHCP.

```console S1
configure terminal
ip arp inspection vlan 1
interface g0/1
ip arp inspection trust
end
```

```console PC1
arp -d
```

```console PC4
arp -d
```

```verify
ping PC1 192.168.1.1 => success
ping PC4 192.168.1.1 => failure
S1# show ip arp inspection
  expect: 1
```

```console PC4
ipconfig /renew
```

```verify
ping PC4 192.168.1.1 => success
```

## Reflection

1. Why is the uplink to R1 trusted for both DHCP snooping and DAI?
2. What could an attacker do with a rogue DHCP server if snooping were off?
3. Port security with `violation restrict` instead of `shutdown`: what would PC2 see?
