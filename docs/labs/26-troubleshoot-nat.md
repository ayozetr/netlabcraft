# Lab 26 — Troubleshooting: NAT overload that does not translate

**Course:** CCNA 3 (Enterprise Networking, Security, and Automation) · **Time:** 45 minutes

## Objectives

- Configure PAT (NAT overload) on a small office router and prove it with a host on the "internet".
- Find three faults — inside and outside swapped, an ACL that does not match the LAN, and a
  missing `overload` — with `show ip nat translations`, `show ip nat statistics`, `show ip
  interface` and `show access-lists`.
- Repair each and check the translation table again.

## Topology

```text
 PC1 ─┐
      S1 ── G0/0  R1  G0/1 ──── G0/0  ISP  G0/1 ── Host1
 PC2 ─┘  192.168.26.0/24   203.0.113.0/30         198.51.100.0/24
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 (inside) | 192.168.26.1/24 |
| R1 | | G0/1 (outside) | 203.0.113.2/30 |
| ISP | Router-2 | G0/0 | 203.0.113.1/30 |
| ISP | | G0/1 | 198.51.100.1/24 |
| PC1 | PC | Fa0 | 192.168.26.10/24, gw 192.168.26.1 |
| PC2 | PC | Fa0 | 192.168.26.11/24, gw 192.168.26.1 |
| Host1 | PC | Fa0 | 198.51.100.10/24, gw 198.51.100.1 |

The ISP routes nothing to private addresses: only translated packets can come back.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "Host1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "ISP", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "Host1", "b": "ISP", "b_port": "g0/1"}
```

```console PC1
ipconfig 192.168.26.10 255.255.255.0 192.168.26.1
```

```console PC2
ipconfig 192.168.26.11 255.255.255.0 192.168.26.1
```

```console Host1
ipconfig 198.51.100.10 255.255.255.0 198.51.100.1
```

```console ISP
enable
configure terminal
interface g0/0
ip address 203.0.113.1 255.255.255.252
no shutdown
interface g0/1
ip address 198.51.100.1 255.255.255.0
no shutdown
end
```

## Part 1 — The working network

PAT: every inside host in ACL 1 shares R1's outside address, told apart by port number.

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.26.1 255.255.255.0
ip nat inside
no shutdown
interface g0/1
ip address 203.0.113.2 255.255.255.252
ip nat outside
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 203.0.113.1
access-list 1 permit 192.168.26.0 0.0.0.255
ip nat inside source list 1 interface g0/1 overload
end
```

```lab
sim.advance {"seconds": 35}
```

```verify
ping PC1 198.51.100.10 => success
ping PC2 198.51.100.10 => success
R1# show ip nat translations
  expect: 203.0.113.2
  expect: 192.168.26.10
  expect: 192.168.26.11
R1# show ip nat statistics
  expect: Inside interfaces:  GigabitEthernet0/0
  expect: Outside interfaces: GigabitEthernet0/1
```

## Part 2 — The faults

Three changes while you were away. Type them as they are.

```console R1
configure terminal
interface g0/0
no ip nat inside
ip nat outside
interface g0/1
no ip nat outside
ip nat inside
end
clear ip nat translation *
clear ip nat statistics
```

## Part 3 — Diagnose and repair

### Fault 1: the interfaces' roles are swapped

```verify
ping PC1 198.51.100.10 => failure
R1# show ip nat translations
  reject: 192.168.26.10
R1# show ip nat statistics
  expect: Inside interfaces:  GigabitEthernet0/1
  expect: Outside interfaces: GigabitEthernet0/0
  expect: Hits: 0 (found a translation)  Misses: 0
R1# show ip interface g0/0
  expect: NAT: outside
```

Both counters are zero: not one packet reached a point where NAT looked at it. (`clear ip nat
statistics` resets them, to watch one test at a time.)

A translation starts when a packet arrives on an inside interface and leaves by an outside one.
Here packets from the LAN arrive on an interface marked outside, so nothing is translated, and
the ISP sees (and cannot answer) a private source address. Put the roles back:

```console R1
configure terminal
interface g0/0
no ip nat outside
ip nat inside
interface g0/1
no ip nat inside
ip nat outside
end
```

```verify
ping PC1 198.51.100.10 => success
R1# show ip nat translations
  expect: 192.168.26.10
```

### Fault 2: the ACL does not match the LAN

Someone "tightens" the ACL:

```console R1
configure terminal
no access-list 1
access-list 1 permit 192.168.62.0 0.0.0.255
end
clear ip nat translation *
clear ip nat statistics
```

```verify
ping PC1 198.51.100.10 => failure
R1# show ip nat translations
  reject: 192.168.26.10
R1# show access-lists 1
  expect: 192.168.62.0 0.0.0.255
  reject: matches)
R1# show ip nat statistics
  expect: access list 1
  expect: Hits: 0 (found a translation)  Misses: 0
```

The roles are right and the rule is there, but ACL 1 describes 192.168.62.0/24 (digits
swapped), a network that does not exist here, so the LAN's packets never match it. Fix the ACL:

```console R1
configure terminal
no access-list 1
access-list 1 permit 192.168.26.0 0.0.0.255
end
```

```verify
ping PC1 198.51.100.10 => success
ping PC2 198.51.100.10 => success
R1# show access-lists 1
  expect: 10 permit 192.168.26.0 0.0.0.255 (
  expect: matches)
```

A rule that is used counts its matches: the ACL's counter moves with each new translation.

### Fault 3: translating to one address without sharing it

The `overload` keyword goes missing. With one outside address and no port translation, only
one inside host can be translated at a time:

```console R1
configure terminal
no ip nat inside source list 1 interface g0/1 overload
ip nat inside source list 1 interface g0/1
end
clear ip nat translation *
```

```verify
ping PC1 198.51.100.10 => success
ping PC2 198.51.100.10 => failure
R1# show ip nat translations
  expect: 192.168.26.10
  reject: 192.168.26.11
R1# show running-config | include ip nat inside source
  reject: overload
```

PC1 got the only outside address; PC2 has nothing left to be translated to. Put `overload` back:

```console R1
configure terminal
no ip nat inside source list 1 interface g0/1
ip nat inside source list 1 interface g0/1 overload
end
clear ip nat translation *
```

```verify
ping PC1 198.51.100.10 => success
ping PC2 198.51.100.10 => success
R1# show ip nat translations
  expect: 192.168.26.10
  expect: 192.168.26.11
R1# show running-config | include ip nat inside source
  expect: overload
```

## Reflection

1. Which side of a translation is the "inside local" address, and which the "inside global"?
2. Why did Fault 1 let the ping leave but never come back?
3. With `overload`, how does R1 tell PC1's reply from PC2's when both share one address?
