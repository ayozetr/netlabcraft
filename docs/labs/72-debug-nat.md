# Lab 72 — Reading `debug ip nat`: translations that never happen

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 40 minutes

## Objectives

- Use `debug ip nat` to see each new translation as it is made, and `debug ip nat detailed` to
  see the ports (or ICMP identifiers) that stand for an inside host on the outside.
- Treat *silence* as a symptom: a router that says nothing about a flow it should translate
  has not matched it with the rule, and the reason is one of two things you can check.
- Find a rule whose access list names the wrong network, and an interface that was never marked
  `ip nat outside`.

## Topology

```text
 PC1 ── (G0/0) R1 (G0/1) ──── (G0/0) ISP (G0/1) ── Server1
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 (inside) | 192.168.72.1/24 |
| R1 | | G0/1 (outside) | 203.0.113.2/30 |
| ISP | Router-2 | G0/0 | 203.0.113.1/30 |
| ISP | | G0/1 | 198.51.100.1/24 |
| PC1 | PC | Fa0 | 192.168.72.10/24, gateway 192.168.72.1 |
| Server1 | Server | G0 | 198.51.100.10/24, gateway 198.51.100.1 |

The provider knows nothing about 192.168.72.0/24: only packets translated to R1's outside
address can be answered.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "Server", "name": "Server1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/1", "b": "ISP", "b_port": "g0/0"}
link.add {"a": "Server1", "b": "ISP", "b_port": "g0/1"}
```

```console PC1
ipconfig 192.168.72.10 255.255.255.0 192.168.72.1
```

```console Server1
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

## Part 1 — A rule with the wrong access list

The rule translates what the access list permits, to the address of the outside interface, and
shares that address by port (`overload`). The access list was typed from a note with a typo in
the third octet.

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.72.1 255.255.255.0
ip nat inside
no shutdown
interface g0/1
ip address 203.0.113.2 255.255.255.252
ip nat outside
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 203.0.113.1
access-list 1 permit 192.168.27.0 0.0.0.255
ip nat inside source list 1 interface g0/1 overload
end
debug ip nat
debug ip nat detailed
```

```verify
ping PC1 198.51.100.10 => failure
R1# show logging
  reject: NAT debug
  reject: NAT detail debug
R1# show debugging
  expect: NAT
  expect: NAT in detail
```

The debugging is on, the ping went through the router, and it printed nothing. A router that
translates says so; this one never matched the packet with the rule, and the packet went on to
the provider with its private source address, which the provider cannot route back.

```verify
R1# show ip nat translations
  reject: 192.168.72.10
R1# show access-lists 1
  expect: 192.168.27.0
```

Two things decide whether a packet is translated: the interfaces (the packet came in on an
inside one and goes out on an outside one) and the access list (its source is permitted). The
interfaces are right here (`ip nat inside` and `ip nat outside` are both set), and the access
list permits 192.168.27.0 where PC1 is on 192.168.72.0. Fix the list.

```console R1
configure terminal
no access-list 1
access-list 1 permit 192.168.72.0 0.0.0.255
end
clear logging
```

```verify
ping PC1 198.51.100.10 => success
R1# show logging
  expect: NAT debug: new translation, source 192.168.72.10 → 203.0.113.2, to 198.51.100.10
  expect: NAT detail debug: icmp inside local 192.168.72.10:
  expect: becomes inside global 203.0.113.2:
  expect: (port translation, overload)
```

The first line says a translation was made, and the second says what stands for what: the
inside host's ICMP identifier on one side, and the same number on R1's own address on the other.
With `overload` the outside address is shared by every inside host, and the identifier (or the
TCP and UDP port) tells the replies apart.

A web page uses a TCP port instead of an ICMP identifier:

```console R1
clear logging
```

```verify
run browser.get {"device": "PC1", "url": "http://198.51.100.10/index.html"}
  expect: 200
R1# show logging
  expect: NAT detail debug: tcp inside local 192.168.72.10:
```

## Part 2 — An interface that is not marked outside

Take the mark off the outside interface, as a half-finished change would.

```console R1
configure terminal
interface g0/1
no ip nat outside
end
clear ip nat translation *
clear logging
```

```verify
ping PC1 198.51.100.10 => failure
R1# show logging
  reject: NAT debug
R1# show running-config
  reject: ip nat outside
  expect: ip nat inside
```

Again nothing is printed, and this time the access list is right. A translation needs both
marks. The running configuration names the interfaces that have them: one inside and no
outside.

```console R1
configure terminal
interface g0/1
ip nat outside
end
```

```verify
ping PC1 198.51.100.10 => success
R1# show logging
  expect: NAT debug: new translation
R1# show ip nat translations
  expect: 203.0.113.2
```

```console R1
undebug all
```

## Reflection

1. Why is a router that prints nothing under `debug ip nat` more informative than it seems?
2. The inside local address and the inside global address are the same in the table and in the
   detailed debug. Which one does Server1 see, and which one does PC1 see?
3. Why does `overload` need a port or an identifier, and what would happen to a second inside
   host without it?
4. Which of the two faults would also stop traffic that is *not* translated?
