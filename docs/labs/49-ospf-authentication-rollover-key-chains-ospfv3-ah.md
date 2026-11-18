# Lab 49 — OSPF authentication: rolling a key over, key chains and OSPFv3 with AH

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 45 minutes

## Objectives

- Protect an OSPF link with a message-digest key and change the key without losing the
  adjacency, by holding two keys while the routers are changed one at a time.
- Put the keys in a key chain with a send lifetime and an accept lifetime, and read what the
  device's clock does to them (`show key chain`, `clock set`).
- Protect OSPFv3 with an IPsec Authentication Header (`ospfv3 authentication ipsec spi …`) and
  read why two routers that disagree stay apart.

The keys here are made up for the lab.

## Topology

```text
 R1 ───────────── R2
   10.49.12.0/24
```

| Device | Model | Interface | Address | Router ID |
|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.49.12.1/24 | 1.1.1.1 |
| R2 | Router-2 | G0/0 | 10.49.12.2/24 | 2.2.2.2 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
link.add {"a": "R1", "a_port": "g0/0", "b": "R2", "b_port": "g0/0"}
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.49.12.1 255.255.255.0
ip ospf authentication message-digest
ip ospf message-digest-key 1 md5 lab-fake-key1
no shutdown
exit
router ospf 1
router-id 1.1.1.1
network 10.49.12.0 0.0.0.255 area 0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.49.12.2 255.255.255.0
ip ospf authentication message-digest
ip ospf message-digest-key 1 md5 lab-fake-key1
no shutdown
exit
router ospf 1
router-id 2.2.2.2
network 10.49.12.0 0.0.0.255 area 0
end
```

```lab
sim.advance {"seconds": 60}
```

## Part 1 — One key at both ends

Both routers sign their packets with key 1 and the neighbours are Full:

```verify
R1# show ip ospf neighbor
  expect: FULL
R1# show ip ospf interface g0/0
  expect: Authentication: message digest, signing with key 1
```

## Part 2 — Rolling the key over

To change the key without a break, the new key goes on one router first. A router that holds
two keys sends one copy of each packet per key, so the neighbour that still has only the old
key keeps hearing it. Put key 2 on R1:

```console R1
configure terminal
interface g0/0
ip ospf message-digest-key 2 md5 lab-fake-key2
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ip ospf neighbor
  expect: FULL
R1# show ip ospf interface g0/0
  expect: signing with key 2 (and 1 older key)
```

Then R2 gets the new key and R1 drops the old one. The adjacency never went down:

```console R2
configure terminal
interface g0/0
ip ospf message-digest-key 2 md5 lab-fake-key2
end
```

```console R1
configure terminal
interface g0/0
no ip ospf message-digest-key 1
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ip ospf neighbor
  expect: FULL
R1# show ip ospf interface g0/0
  expect: Authentication: message digest, signing with key 2
  reject: older key
```

## Part 3 — A key chain with lifetimes

A key chain holds keys that are valid for a time. R1 will use a chain whose key 2 may only sign
from the year 2030. The simulated clock starts in 2026, so nothing signs yet and the adjacency
is lost:

```console R1
configure terminal
key chain RING
key 2
key-string lab-fake-key2
send-lifetime 00:00:00 Jan 1 2030 infinite
accept-lifetime 00:00:00 Jan 1 2020 infinite
exit
exit
interface g0/0
ip ospf authentication key-chain RING
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show key chain RING
  expect: not yet valid
  expect: no key signs now
R1# show ip ospf neighbor
  reject: FULL
```

Set R1's clock past the start of the lifetime. The key signs, and R2, which holds the same key
by hand, hears it again:

```console R1
clock set 12:00:00 2 Jan 2030
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show key chain RING
  expect: signing now with key 2
R1# show ip ospf neighbor
  expect: FULL
R1# show ip ospf interface g0/0
  expect: key chain RING, signing with key 2
```

## Part 4 — OSPFv3 and an Authentication Header

OSPFv3 has no authentication of its own: it relies on IPsec. `ospfv3 authentication ipsec spi N
md5|sha1 KEY` wraps every OSPFv3 packet of the interface in an Authentication Header, and the
two ends need the same SPI, algorithm and key (32 hexadecimal digits for md5, 40 for sha1).
Start OSPFv3 on the link with R1 protected and R2 not:

```console R1
configure terminal
ipv6 unicast-routing
ipv6 router ospf 1
router-id 1.1.1.1
exit
interface g0/0
ipv6 enable
ipv6 ospf 1 area 0
ospfv3 authentication ipsec spi 256 md5 0123456789abcdef0123456789abcdef
end
```

```console R2
configure terminal
ipv6 unicast-routing
ipv6 router ospf 1
router-id 2.2.2.2
exit
interface g0/0
ipv6 enable
ipv6 ospf 1 area 0
end
```

```verify
R1# show ipv6 ospf neighbor
  expect: Not a neighbour: 2.2.2.2
  expect: no OSPFv3 authentication
  reject: FULL
R2# show ipv6 ospf neighbor
  expect: no AH header
```

Give R2 the same setting:

```console R2
configure terminal
interface g0/0
ospfv3 authentication ipsec spi 256 md5 0123456789abcdef0123456789abcdef
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show ipv6 ospf neighbor
  expect: FULL
R1# show ipv6 ospf
  expect: authentication: AH md5 with SPI 256
```

## What you learned

- Several `ip ospf message-digest-key` lines on an interface are signed as one copy per key and
  any held key is accepted, so a key is changed in steps (new key on one router, then the other,
  then the old key off) without a break.
- A key chain gives each key a send lifetime and an accept lifetime against the device's clock;
  `show key chain` says which keys are valid now and which one signs.
- OSPFv3 is protected by an IPsec Authentication Header; two routers become neighbours only when
  the SPI, the algorithm and the key agree, and `show ipv6 ospf neighbor` says which of them
  rejects the other.
