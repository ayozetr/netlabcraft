# Lab 55 — IPsec: perfect forward secrecy that does not match, and transport against tunnel mode

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Build a site-to-site IPsec tunnel that asks for perfect forward secrecy (`set pfs group14`),
  and read the group on both ends.
- Make the two ends disagree about PFS, read the refusal and which router says it, and repair it.
- Switch the transform set to `mode transport`, and see why it protects only traffic between the
  two routers themselves, not the LANs behind them.

## Topology

```text
 PC1 ── (G0/0) R1 (G0/1) ──── (G0/0) ISP (G0/1) ──── (G0/1) R2 (G0/0) ── PC2
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| R1 | | G0/1 | 203.0.113.1/30 |
| ISP | Router-2 | G0/0 | 203.0.113.2/30 |
| ISP | | G0/1 | 198.51.100.2/30 |
| R2 | Router-2 | G0/1 | 198.51.100.1/30 |
| R2 | | G0/0 | 192.168.2.1/24 |
| PC1 | PC | Fa0 | 192.168.1.10/24 |
| PC2 | PC | Fa0 | 192.168.2.10/24 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "ISP", "b_port": "g0/0"}
link.add {"a": "ISP", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/0"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC2
ipconfig 192.168.2.10 255.255.255.0 192.168.2.1
```

```console ISP
enable
configure terminal
interface g0/0
ip address 203.0.113.2 255.255.255.252
no shutdown
interface g0/1
ip address 198.51.100.2 255.255.255.252
no shutdown
end
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 203.0.113.1 255.255.255.252
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 203.0.113.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 192.168.2.1 255.255.255.0
no shutdown
interface g0/1
ip address 198.51.100.1 255.255.255.252
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 198.51.100.2
end
```

## Part 1 — A tunnel with PFS

Both sites use the same pre-shared key, policy and transform set (every key in this lab is a
made-up one). The crypto map adds `set pfs group14`: every new set of IPsec keys comes from a
fresh Diffie-Hellman exchange in quick mode, so that a stolen ISAKMP key cannot open the traffic
that was protected before.

```console R1
configure terminal
crypto isakmp policy 10
encryption aes 256
hash sha
authentication pre-share
group 14
exit
crypto isakmp key VpnKey2026 address 198.51.100.1
crypto ipsec transform-set TS esp-aes 256 esp-sha256-hmac
exit
access-list 110 permit ip 192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255
crypto map VPN 10 ipsec-isakmp
set peer 198.51.100.1
set transform-set TS
set pfs group14
match address 110
exit
interface g0/1
crypto map VPN
end
```

```console R2
configure terminal
crypto isakmp policy 10
encryption aes 256
hash sha
authentication pre-share
group 14
exit
crypto isakmp key VpnKey2026 address 203.0.113.1
crypto ipsec transform-set TS esp-aes 256 esp-sha256-hmac
exit
access-list 110 permit ip 192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255
crypto map VPN 10 ipsec-isakmp
set peer 203.0.113.1
set transform-set TS
set pfs group14
match address 110
exit
interface g0/1
crypto map VPN
end
```

```verify
ping PC1 192.168.2.10 => success
R1# show crypto map
  expect: PFS: group14
R1# show crypto ipsec sa
  expect: PFS: group14 (a fresh Diffie-Hellman exchange gave these keys)
R2# show crypto ipsec sa
  expect: PFS: group14 (a fresh Diffie-Hellman exchange gave these keys)
```

## Part 2 — The ends disagree

A technician removes PFS from R2 only. Quick mode carries the group, so R2 refuses R1's offer:
the proposals are the same except for the group.

```console R2
configure terminal
crypto map VPN 10 ipsec-isakmp
no set pfs
end
clear crypto sa
```

```console R1
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

```verify
ping PC1 192.168.2.10 => failure
R2# show crypto isakmp sa
  expect: failed
  expect: PFS differs
R1# show crypto ipsec sa
  expect: PFS differs
```

The message names both views: "PFS group14 at the peer, no PFS here". Which end is right is a policy
decision: PFS is the stronger choice. Put it back on R2 and clear both ends.

```console R2
configure terminal
crypto map VPN 10 ipsec-isakmp
set pfs group14
end
clear crypto sa
```

```console R1
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

```verify
ping PC1 192.168.2.10 => success
R1# show crypto ipsec sa
  expect: PFS: group14 (a fresh Diffie-Hellman exchange gave these keys)
```

## Part 3 — Transport mode

`mode transport` leaves the packet's own IP header in place and protects only its payload. It is
meant for traffic between the two peers themselves (a management session, a GRE tunnel), not for
whole LANs. A new transform set `TST` is used on both routers, still for the LANs' access list.

```console R1
configure terminal
crypto ipsec transform-set TST esp-aes 256 esp-sha256-hmac
mode transport
exit
crypto map VPN 10 ipsec-isakmp
set transform-set TST
end
clear crypto sa
```

```console R2
configure terminal
crypto ipsec transform-set TST esp-aes 256 esp-sha256-hmac
mode transport
exit
crypto map VPN 10 ipsec-isakmp
set transform-set TST
end
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

```verify
ping PC1 192.168.2.10 => failure
R1# show crypto map
  expect: transport mode
```

The association comes up, and the LAN packets are dropped with a reason: transport mode protects only
traffic between this router and its peer. A real router would fall back to a tunnel at negotiation;
here the reason is what you read. Change the access list to the routers' own addresses.

```console R1
configure terminal
no access-list 110
access-list 110 permit ip host 203.0.113.1 host 198.51.100.1
end
clear crypto sa
```

```console R2
configure terminal
no access-list 110
access-list 110 permit ip host 198.51.100.1 host 203.0.113.1
end
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

```verify
ping R1 198.51.100.1 => success
R1# show crypto ipsec sa
  expect: Mode: transport
  expect: PFS: group14
```

## What you learned

- PFS means a fresh exchange for every set of IPsec keys, and both ends must ask for it with the
  same group: the refusal says `PFS differs`.
- Transport mode is for traffic between the two peers; site-to-site VPNs of LANs use tunnel mode.
