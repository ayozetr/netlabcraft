# Lab 25 — Troubleshooting: hosts that get no address, or the wrong one

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Run a DHCP server on one router for its own LAN and, through a relay, for a LAN behind another router.
- Find three faults — a missing `ip helper-address`, an excluded range that swallows the pool,
  and a pool that hands out the wrong gateway — from `show ip dhcp pool`, `show ip dhcp
  binding`, `show ip dhcp server statistics`, `show ip interface` and the host's own `ipconfig`.
- Repair each and renew the host's lease.

## Topology

```text
 PC1 ── G0/0  R1  G0/1 ───── G0/0  R2  G0/1 ── PC2
   LAN A      10.25.0.0/30           LAN B
 10.25.1.0/24                      10.25.2.0/24
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.25.1.1/24 |
| R1 | | G0/1 | 10.25.0.1/30 |
| R2 | Router-2 | G0/0 | 10.25.0.2/30 |
| R2 | | G0/1 | 10.25.2.1/24 |
| PC1 | PC | Fa0 | DHCP (LAN A) |
| PC2 | PC | Fa0 | DHCP (LAN B) |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/1"}
```

## Part 1 — The working network

R1 is the DHCP server for both LANs. The first ten addresses of each are kept for fixed devices.
R2 forwards LAN B's broadcasts to R1 as unicasts; its own LAN address in the request tells R1
which pool to answer from.

```console R1
enable
configure terminal
interface g0/0
ip address 10.25.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.25.0.1 255.255.255.252
no shutdown
exit
ip route 10.25.2.0 255.255.255.0 10.25.0.2
ip dhcp excluded-address 10.25.1.1 10.25.1.10
ip dhcp excluded-address 10.25.2.1 10.25.2.10
ip dhcp pool LAN-A
network 10.25.1.0 255.255.255.0
default-router 10.25.1.1
dns-server 10.25.1.5
exit
ip dhcp pool LAN-B
network 10.25.2.0 255.255.255.0
default-router 10.25.2.1
dns-server 10.25.1.5
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.25.0.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.25.2.1 255.255.255.0
ip helper-address 10.25.0.1
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 10.25.0.1
end
```

```console PC1
ipconfig /renew
```

```console PC2
ipconfig /renew
```

```verify
PC1> ipconfig
  expect: 10.25.1.11
  expect: 10.25.1.1
PC2> ipconfig
  expect: 10.25.2.11
  expect: 10.25.2.1
ping PC1 10.25.2.11 => success
R1# show ip dhcp binding
  expect: 10.25.1.11
  expect: 10.25.2.11
R2# show ip interface g0/1
  expect: DHCP relay to: 10.25.0.1
```

## Part 2 — The faults

Overnight, three changes: R2 loses its relay line, LAN A's excluded range is "extended", and
LAN B's pool names a different gateway.

```console R2
configure terminal
interface g0/1
no ip helper-address
end
```

```console R1
configure terminal
ip dhcp excluded-address 10.25.1.11 10.25.1.254
ip dhcp pool LAN-B
default-router 10.25.2.254
end
```

Both hosts give up their leases:

```console PC1
ipconfig /release
```

```console PC2
ipconfig /release
```

```console R1
clear ip dhcp binding *
```

## Part 3 — Diagnose and repair

### PC2: no answer at all

Reset R1's message counters, so what they show comes from this attempt only:

```console R1
clear ip dhcp server statistics
```

```verify
PC2> ipconfig /renew
  expect: DHCP failed
R2# show ip interface g0/1
  reject: DHCP relay
R1# show ip dhcp server statistics
  expect: DISCOVER     0
```

PC2's broadcast stops at R2: nothing there forwards it, and R1 counted no Discover at all.
Put the relay back.

```console R2
configure terminal
interface g0/1
ip helper-address 10.25.0.1
end
```

### PC1: the server hears it, and has nothing to give

```verify
PC1> ipconfig /renew
  expect: DHCP failed
R1# show ip dhcp pool LAN-A
  expect: 0 free
R1# show running-config | include excluded
  expect: 10.25.1.11 10.25.1.254
R1# show ip dhcp server statistics
  reject: DISCOVER     0
  expect: OFFER        0
```

R1 gets LAN A's Discover (the counter moved) but sends no Offer: every address in the pool is excluded. Remove the excluded range
that swallows the pool; the first ten stay reserved.

```console R1
configure terminal
no ip dhcp excluded-address 10.25.1.11 10.25.1.254
end
```

### PC2: an address, but no way out

PC2 now gets a lease. Does it work?

```console PC2
ipconfig /renew
```

```verify
PC2> ipconfig
  expect: 10.25.2.11
  expect: 10.25.2.254
ping PC2 10.25.2.1 => success
ping PC2 10.25.1.1 => failure
R1# show ip dhcp pool LAN-B
  expect: Default router   10.25.2.254
```

The lease is good, but the gateway it names (10.25.2.254) is nobody. Local traffic works; any
other network is unreachable. Fix the pool, then renew:

```console R1
configure terminal
ip dhcp pool LAN-B
default-router 10.25.2.1
end
```

```console PC1
ipconfig /renew
```

```console PC2
ipconfig /release
ipconfig /renew
```

```verify
PC1> ipconfig
  expect: 10.25.1.11
PC2> ipconfig
  expect: 10.25.2.1
ping PC1 10.25.2.11 => success
ping PC2 10.25.1.11 => success
R1# show ip dhcp binding
  expect: 10.25.1.11
  expect: 10.25.2.11
```

## Reflection

1. Why can R1 pick the right pool for PC2 even though PC2's request arrives from R2?
2. A host shows an address starting 169.254: what does that tell you about the DHCP exchange?
3. Why did the repair need `ipconfig /release` before the second renew on PC2?
