# Lab 17 — Site-to-site VPN: GRE over IPsec

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Join two sites across a provider with a GRE tunnel, and run OSPF through it.
- Protect the tunnel with IPsec: an ISAKMP policy and pre-shared key (phase 1), a transform
  set (phase 2), and a crypto map that encrypts the GRE packets between the two routers.
- Check that the provider only ever sees ESP between the sites' public addresses.

## Topology

```text
 PC1 ── (G0/0) R1 (G0/1) ──── (G0/0) ISP (G0/1) ──── (G0/1) R2 (G0/0) ── PC2
              Tunnel0 10.9.9.1  ═══════════════════  Tunnel0 10.9.9.2
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| R1 | | G0/1 | 203.0.113.1/30 |
| R1 | | Tunnel0 | 10.9.9.1/30 |
| ISP | Router-2 | G0/0 | 203.0.113.2/30 |
| ISP | | G0/1 | 198.51.100.2/30 |
| R2 | Router-2 | G0/1 | 198.51.100.1/30 |
| R2 | | G0/0 | 192.168.2.1/24 |
| R2 | | Tunnel0 | 10.9.9.2/30 |
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

## Part 1 — The provider and the edges

The provider routes only the public addresses; it knows nothing of either LAN.

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

```verify
ping R1 198.51.100.1 => success
ping PC1 192.168.2.10 => failure
```

## Part 2 — The GRE tunnel and OSPF through it

A tunnel interface has its own subnet. Its packets are wrapped in a new IP header from the
tunnel source to the tunnel destination, so the provider only routes the outer header.

```console R1
configure terminal
interface tunnel 0
ip address 10.9.9.1 255.255.255.252
tunnel source g0/1
tunnel destination 198.51.100.1
exit
router ospf 1
router-id 1.1.1.1
network 192.168.1.0 0.0.0.255 area 0
network 10.9.9.0 0.0.0.3 area 0
passive-interface g0/0
end
```

```console R2
configure terminal
interface tunnel 0
ip address 10.9.9.2 255.255.255.252
tunnel source g0/1
tunnel destination 203.0.113.1
exit
router ospf 1
router-id 2.2.2.2
network 192.168.2.0 0.0.0.255 area 0
network 10.9.9.0 0.0.0.3 area 0
passive-interface g0/0
end
```

```lab
sim.advance {"seconds": 40}
```

```verify
R1# show interfaces tunnel 0
  expect: 198.51.100.1
R1# show ip ospf neighbor
  expect: 2.2.2.2
  expect: Tunnel0
R1# show ip route
  expect: 192.168.2.0/24
  expect: Tunnel0
ping PC1 192.168.2.10 => success
```

GRE alone hides nothing: in Simulation mode, the provider sees the inner addresses
(192.168.1.10 → 192.168.2.10) in every packet.

## Part 3 — IPsec around the tunnel

Phase 1 (ISAKMP) agrees how the routers authenticate each other and protect their talks:
AES-256, SHA, a pre-shared key and Diffie-Hellman group 14. Phase 2 (the transform set)
agrees how user traffic is protected: ESP with AES and SHA. The crypto map ties it together:
the peer, the transform set and an ACL saying what to encrypt — here every GRE packet
between the two routers' public addresses.

```console R1
configure terminal
crypto isakmp policy 10
encryption aes 256
hash sha
authentication pre-share
group 14
exit
crypto isakmp key VpnKey2026 address 198.51.100.1
crypto ipsec transform-set GRE-TS esp-aes esp-sha-hmac
exit
access-list 120 permit gre host 203.0.113.1 host 198.51.100.1
crypto map GRE-VPN 10 ipsec-isakmp
set peer 198.51.100.1
set transform-set GRE-TS
match address 120
exit
interface g0/1
crypto map GRE-VPN
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
crypto ipsec transform-set GRE-TS esp-aes esp-sha-hmac
exit
access-list 120 permit gre host 198.51.100.1 host 203.0.113.1
crypto map GRE-VPN 10 ipsec-isakmp
set peer 203.0.113.1
set transform-set GRE-TS
match address 120
exit
interface g0/1
crypto map GRE-VPN
end
```

The first GRE packet starts the negotiation (ISAKMP on UDP 500), then traffic flows as ESP:

```lab
sim.clear_events {}
```

```verify
ping PC1 192.168.2.10 => success
events: ESP
events: ISAKMP
R1# show crypto isakmp sa
  expect: 198.51.100.1
  expect: established
R1# show crypto ipsec sa
  expect: Peer 198.51.100.1
  expect: encapsulated
R1# show ip ospf neighbor
  expect: 2.2.2.2
ping PC2 192.168.1.10 => success
```

## Reflection

1. Why is GRE needed at all, when IPsec could protect the LAN-to-LAN traffic directly?
   (Think about OSPF hellos, which are multicast.)
2. Why does the crypto ACL name the routers' public addresses and not the LANs?
3. What would the provider see in a packet from PC1 to PC2 before Part 3? After?
