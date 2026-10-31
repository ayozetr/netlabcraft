# Lab 9 — DHCPv4 server and relay, and the three ways to give IPv6 addresses

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 60 minutes

## Objectives

- Run a DHCPv4 server on a router with excluded addresses, and serve a LAN behind another
  router through a relay agent (`ip helper-address`).
- Give IPv6 hosts their addresses three ways:
  - **SLAAC** only: the router advertisement carries the prefix and nothing else;
  - **stateless DHCPv6**: SLAAC for the address, DHCPv6 for the DNS server and domain
    (the *other configuration* flag);
  - **stateful DHCPv6**: the address itself comes from a DHCPv6 server (the *managed* flag).

## Topology

```text
 PC1 ── S1 ──G0/0 ┐
                  R1 ──G0/2──────G0/0── R2 ──G0/1── S3 ── PC3
 PC2 ── S2 ──G0/1 ┘
```

| Device | Interface | IPv4 | IPv6 | How hosts get addresses |
|---|---|---|---|---|
| R1 (Router-3) | G0/0 | 192.168.10.1/24 | 2001:db8:acad:10::1/64 | DHCPv4 from R1 · SLAAC |
| R1 | G0/1 | 192.168.20.1/24 | 2001:db8:acad:20::1/64 | DHCPv4 from R1 · stateless DHCPv6 |
| R1 | G0/2 | 10.0.0.1/30 | 2001:db8:acad:ff::1/64 | — |
| R2 (Router-2) | G0/0 | 10.0.0.2/30 | 2001:db8:acad:ff::2/64 | — |
| R2 | G0/1 | 192.168.30.1/24 | 2001:db8:acad:30::1/64 | DHCPv4 from R1 via relay · stateful DHCPv6 from R2 |

PC1, PC2 and PC3 are PCs; S1–S3 are Switch-24.

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "Switch-24", "name": "S3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "S2", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/2", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "S3", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S2", "b_port": "fa0/1"}
link.add {"a": "PC3", "b": "S3", "b_port": "fa0/1"}
```

## Part 1 — Addresses and routes

```console R1
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ip address 192.168.10.1 255.255.255.0
ipv6 address 2001:db8:acad:10::1/64
no shutdown
interface g0/1
ip address 192.168.20.1 255.255.255.0
ipv6 address 2001:db8:acad:20::1/64
no shutdown
interface g0/2
ip address 10.0.0.1 255.255.255.252
ipv6 address 2001:db8:acad:ff::1/64
no shutdown
exit
ip route 192.168.30.0 255.255.255.0 10.0.0.2
ipv6 route 2001:db8:acad:30::/64 2001:db8:acad:ff::2
end
```

```console R2
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ip address 10.0.0.2 255.255.255.252
ipv6 address 2001:db8:acad:ff::2/64
no shutdown
interface g0/1
ip address 192.168.30.1 255.255.255.0
ipv6 address 2001:db8:acad:30::1/64
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 10.0.0.1
ipv6 route ::/0 2001:db8:acad:ff::1
end
```

## Part 2 — DHCPv4 server and relay

R1 serves all three LANs. The first ten addresses of each are kept for servers and printers.

```console R1
configure terminal
ip dhcp excluded-address 192.168.10.1 192.168.10.10
ip dhcp excluded-address 192.168.20.1 192.168.20.10
ip dhcp excluded-address 192.168.30.1 192.168.30.10
ip dhcp pool LAN10
network 192.168.10.0 255.255.255.0
default-router 192.168.10.1
dns-server 192.168.10.5
ip dhcp pool LAN20
network 192.168.20.0 255.255.255.0
default-router 192.168.20.1
dns-server 192.168.10.5
ip dhcp pool LAN30
network 192.168.30.0 255.255.255.0
default-router 192.168.30.1
dns-server 192.168.10.5
end
```

DHCP Discovers are broadcasts and don't cross routers. R2 relays them: it forwards each one
to R1 as a unicast, with its own G0/1 address in the `giaddr` field, which tells R1 which
pool to use.

```console R2
configure terminal
interface g0/1
ip helper-address 10.0.0.1
end
```

```console PC1
ipconfig /renew
```

```console PC2
ipconfig /renew
```

```console PC3
ipconfig /renew
```

```verify
PC1> ipconfig
  expect: 192.168.10.11
  expect: 192.168.10.1
PC3> ipconfig
  expect: 192.168.30.11
  expect: 192.168.30.1
R1# show ip dhcp binding
  expect: 192.168.10.11
  expect: 192.168.20.11
  expect: 192.168.30.11
ping PC1 192.168.30.11 => success
```

## Part 3 — SLAAC on LAN 10

Nothing more is needed on R1: with `ipv6 unicast-routing` its G0/0 advertises the prefix,
and PC1 builds its own address from it.

```console PC1
ipv6config auto
```

```verify
PC1> ipconfig
  expect: 2001:db8:acad:10:
```

## Part 4 — Stateless DHCPv6 on LAN 20

The pool holds only the DNS server and the domain. Setting the *other configuration* flag
tells hosts to keep their SLAAC address and ask DHCPv6 for the rest.

```console R1
configure terminal
ipv6 dhcp pool STATELESS
dns-server 2001:db8:acad:10::5
domain-name lab.local
exit
interface g0/1
ipv6 nd other-config-flag
ipv6 dhcp server STATELESS
end
```

```console PC2
ipv6config auto
```

```verify
PC2> ipconfig
  expect: 2001:db8:acad:20:
  expect: 2001:db8:acad:10::5
R1# show ipv6 dhcp pool
  expect: STATELESS
```

## Part 5 — Stateful DHCPv6 on LAN 30

R2 hands out the addresses themselves. The *managed* flag tells hosts to take their address
from DHCPv6; the router advertisement still gives them their default gateway.

```console R2
configure terminal
ipv6 dhcp pool STATEFUL
address prefix 2001:db8:acad:30::/64
dns-server 2001:db8:acad:10::5
domain-name lab.local
exit
interface g0/1
ipv6 nd managed-config-flag
ipv6 dhcp server STATEFUL
end
```

```console PC3
ipv6config auto
```

```verify
PC3> ipconfig
  expect: 2001:db8:acad:30:
R2# show ipv6 dhcp binding
  expect: 2001:db8:acad:30:
ping PC3 2001:db8:acad:10::1 => success
```

## Reflection

1. Why does R1 need a route back to 192.168.30.0/24 before PC3 can get an address?
2. In stateless DHCPv6, which device chooses the host's address? In stateful?
3. What would PC2 do if the router advertisement had neither flag set?
