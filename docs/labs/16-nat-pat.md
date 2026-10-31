# Lab 16 — NAT: static, dynamic pool and PAT

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 50 minutes

## Objectives

- Publish an inside server on a public address with static NAT.
- Share a small pool of public addresses with dynamic NAT, then share a single address
  among every host with PAT (overload).
- Read the translation table and the NAT statistics.

## Topology

```text
 PC1, PC2, Web1 ── S1 ── (G0/0) R1 (G0/1) ── (G0/0) ISP (G0/1) ── Host1 ("the Internet")
          inside: 192.168.1.0/24      209.165.200.224/29       198.51.100.0/24
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 (inside) | 192.168.1.1/24 |
| R1 | | G0/1 (outside) | 209.165.200.225/29 |
| ISP | Router-2 | G0/0 | 209.165.200.230/29 |
| ISP | | G0/1 | 198.51.100.1/24 |
| PC1 | PC | Fa0 | 192.168.1.10/24 |
| PC2 | PC | Fa0 | 192.168.1.11/24 |
| Web1 | Server | G0 | 192.168.1.100/24, published as 209.165.200.226 |
| Host1 | PC | Fa0 | 198.51.100.10/24 |

The ISP has given the company 209.165.200.224/29: .225 for R1, .226 for the web server,
.227–.228 for the pool, and .230 for the ISP itself.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "Server", "name": "Web1"}
device.add {"model": "PC", "name": "Host1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "ISP", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "Web1", "b": "S1", "b_port": "fa0/3"}
link.add {"a": "Host1", "b": "ISP", "b_port": "g0/1"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC2
ipconfig 192.168.1.11 255.255.255.0 192.168.1.1
```

```console Web1
ipconfig 192.168.1.100 255.255.255.0 192.168.1.1
```

```console Host1
ipconfig 198.51.100.10 255.255.255.0 198.51.100.1
```

The ISP knows nothing about private addresses: it only routes the public block to R1.

```console ISP
enable
configure terminal
interface g0/0
ip address 209.165.200.230 255.255.255.248
no shutdown
interface g0/1
ip address 198.51.100.1 255.255.255.0
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
ip address 209.165.200.225 255.255.255.248
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 209.165.200.230
end
```

Without NAT, the inside hosts' packets reach Host1 but the replies have nowhere to go:

```verify
ping PC1 198.51.100.10 => failure
```

## Part 1 — Static NAT for the web server

```console R1
configure terminal
interface g0/0
ip nat inside
interface g0/1
ip nat outside
exit
ip nat inside source static 192.168.1.100 209.165.200.226
end
```

```verify
ping Host1 209.165.200.226 => success
ping Host1 192.168.1.100 => failure
R1# show ip nat translations
  expect: 209.165.200.226
  expect: 192.168.1.100
```

## Part 2 — Dynamic NAT from a pool

Two public addresses for the inside hosts, handed out first come, first served. An ACL picks
the inside addresses that may be translated.

```console R1
configure terminal
access-list 1 permit 192.168.1.0 0.0.0.255
ip nat pool PUBLIC 209.165.200.227 209.165.200.228 netmask 255.255.255.248
ip nat inside source list 1 pool PUBLIC
end
```

```verify
ping PC1 198.51.100.10 => success
ping PC2 198.51.100.10 => success
R1# show ip nat translations
  expect: 209.165.200.227
  expect: 209.165.200.228
R1# show ip nat statistics
  expect: PUBLIC
```

## Part 3 — PAT: one address for everyone

A pool of two runs out at the third host. With `overload`, every inside host shares R1's
outside address, told apart by port (ICMP query ID) numbers.

```console R1
clear ip nat translation *
configure terminal
no ip nat inside source list 1 pool PUBLIC
ip nat inside source list 1 interface g0/1 overload
end
```

```verify
ping PC1 198.51.100.10 => success
ping PC2 198.51.100.10 => success
R1# show ip nat translations
  expect: 209.165.200.225
  expect: 192.168.1.10
  expect: 192.168.1.11
ping Host1 209.165.200.226 => success
```

## Reflection

1. Why can Host1 start a conversation with Web1 but not with PC1?
2. What happens with dynamic NAT when a third inside host tries to reach the Internet while
   the pool's two addresses are in use?
3. With PAT, how does R1 know which inside host a reply belongs to?
