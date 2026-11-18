# Lab 61 — Troubleshooting: a site-to-site IPsec VPN that will not come up

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Build a site-to-site IPsec tunnel between two routers across a provider, with a crypto ACL, an
  ISAKMP policy, a transform set and a crypto map.
- Find three faults from their symptoms: a pre-shared key that differs by one character, a
  transform set that differs between the sites, and a crypto ACL that does not match the traffic.
- Read `show crypto isakmp sa`, `show crypto ipsec sa` and `show logging`, and know which router
  to ask: the one that refused the exchange is the one that says why.
- Clear the failed state with `clear crypto sa` and test again.

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

The provider knows only the two public addresses. The VPN must carry traffic between the two
LANs, and nothing else. Every key in this lab is a made-up one.

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

## Part 1 — The underlay

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

The routers reach each other's public addresses, and the LANs cannot reach each other: the
provider has no route to a private network.

```verify
ping R1 198.51.100.1 => success
ping PC1 192.168.2.10 => failure
R1# show crypto isakmp sa
  expect: No IKE peers yet
```

## Part 2 — The VPN, with three mistakes in it

The configuration below is what a technician typed for both sites from one template. Read it
before you run it: each router names the other's public address as its peer and a crypto ACL
for the traffic to protect. Three things are wrong, and they show one after the other, because
the exchange stops at the first one.

```console R1
configure terminal
crypto isakmp policy 10
encryption aes 256
hash sha
authentication pre-share
group 14
exit
crypto isakmp key VpnKey2026 address 198.51.100.1
crypto ipsec transform-set TS esp-aes esp-sha-hmac
exit
access-list 110 permit ip 192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255
crypto map VPN 10 ipsec-isakmp
set peer 198.51.100.1
set transform-set TS
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
crypto isakmp key VpnKey2O26 address 203.0.113.1
crypto ipsec transform-set TS esp-3des esp-md5-hmac
exit
access-list 110 permit ip 192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255
crypto map VPN 10 ipsec-isakmp
set peer 203.0.113.1
set transform-set TS
match address 110
exit
interface g0/1
crypto map VPN
end
```

## Part 3 — Fault 1: the key

The first packet from PC1 matches R1's crypto ACL and starts an IKE exchange. It fails, so the
ping fails. Which router knows why? The one that refused: R2 checked the proof that R1 sent
and it did not fit.

```verify
ping PC1 192.168.2.10 => failure
R2# show crypto isakmp sa
  expect: 203.0.113.1
  expect: failed
  expect: the pre-shared keys differ
R2# show logging
  expect: IKE with 203.0.113.1 failed
```

Look at the two keys in the configuration: `VpnKey2026` and `VpnKey2O26` (a capital O for
the zero). A key that differs in one character fails exactly like one that differs in all of
them, and the message does not say which one is right. Retype it on R2 from the same source as
R1's, and clear the failed state on both routers before testing again.

```console R2
configure terminal
no crypto isakmp key VpnKey2O26 address 203.0.113.1
crypto isakmp key VpnKey2026 address 203.0.113.1
end
clear crypto sa
```

```console R1
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

## Part 4 — Fault 2: the transform set

Phase 1 works now, so the next failure comes later in the exchange: the two routers agree on
how to protect the exchange itself, and then disagree on how to protect the traffic.

```verify
ping PC1 192.168.2.10 => failure
R2# show crypto isakmp sa
  expect: failed
  expect: transform sets differ
  reject: the pre-shared keys differ
R2# show logging
  expect: refused quick mode message 1
```

The refusal names both transform sets: R1 offers `esp-aes esp-sha-hmac` and R2 only accepts
`esp-3des esp-md5-hmac`. Which one is right is a policy decision (AES with SHA is the stronger
choice, and what the other sites use); both ends must say the same thing.

```console R2
configure terminal
crypto ipsec transform-set TS esp-aes esp-sha-hmac
end
clear crypto sa
```

```console R1
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

## Part 5 — Fault 3: the crypto ACL

Both sites now agree, and the ping still fails. Look at R1's ACL: it names the LAN behind R1 as
the source and the LAN behind R2 as the destination, and that is right for R1. Now read R2's
ACL and compare the two lines: they must be mirror images of each other. Someone typed
the same networks in the same order on both.

```console R1
show access-lists 110
```

For a clearer picture of what a mismatch does, break R1's ACL on purpose: make it name a
network nobody uses. Traffic from PC1 then matches no crypto map entry, so R1 sends it in
the clear to the provider, which has no route for a private address and drops it. There is no
IKE exchange at all, so there is no failure message to read.

```console R1
configure terminal
no access-list 110
access-list 110 permit ip 192.168.9.0 0.0.0.255 192.168.2.0 0.0.0.255
end
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

```verify
ping PC1 192.168.2.10 => failure
R1# show crypto isakmp sa
  expect: No IKE peers yet
R1# show crypto ipsec sa
  expect: No IPsec security associations
```

The signs of a crypto ACL that matches nothing are what is missing: no peer in `show crypto
isakmp sa`, no counters in `show crypto ipsec sa`. Fix R1's ACL so that it is the mirror of R2's
and the tunnel comes up.

```console R1
configure terminal
no access-list 110
access-list 110 permit ip 192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255
end
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

```verify
ping PC1 192.168.2.10 => success
ping PC2 192.168.1.10 => success
R1# show crypto isakmp sa
  expect: 198.51.100.1
  expect: established
R1# show crypto ipsec sa
  expect: Peer 198.51.100.1
  expect: encapsulated
R2# show crypto isakmp sa
  expect: 203.0.113.1
  expect: established
```

## Reflection

1. R1 started the exchange and R2 explained the failure. Why does the initiator often learn
   less than the responder?
2. A crypto ACL says what to protect, not who the peer is. What happens to traffic that matches
   no entry?
3. Why do the two crypto ACLs have to be mirror images of each other?
4. Why did the failure of the transform sets appear only after the key was fixed?
