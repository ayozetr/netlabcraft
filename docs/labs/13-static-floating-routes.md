# Lab 13 — Static, default and floating static routes (IPv4 and IPv6)

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 50 minutes

## Objectives

- Write next-hop static routes for IPv4 and IPv6, and a default route.
- Add a floating static route over a backup serial link, with a higher administrative
  distance, so it only takes over when the main link fails.
- Watch the routing table change when the main link goes down and comes back.

## Topology

```text
                  G0/1 ─────────────── G0/1        (main link, Ethernet)
 PC1 ── G0/0  R1                          R2  G0/0 ── PC2
                  S0/0/0 (DCE) ─────── S0/0/0      (backup link, serial)
```

| Device | Model | Interface | IPv4 | IPv6 |
|---|---|---|---|---|
| R1 | Router-WAN | G0/0 | 192.168.1.1/24 | 2001:db8:1:1::1/64 |
| R1 | | G0/1 | 10.0.12.1/30 | 2001:db8:12::1/64 |
| R1 | | S0/0/0 | 10.9.12.1/30 | 2001:db8:912::1/64 |
| R2 | Router-WAN | G0/0 | 192.168.2.1/24 | 2001:db8:1:2::1/64 |
| R2 | | G0/1 | 10.0.12.2/30 | 2001:db8:12::2/64 |
| R2 | | S0/0/0 | 10.9.12.2/30 | 2001:db8:912::2/64 |
| PC1 | PC | Fa0 | 192.168.1.10/24 | 2001:db8:1:1::10/64 |
| PC2 | PC | Fa0 | 192.168.2.10/24 | 2001:db8:1:2::10/64 |

```lab
device.add {"model": "Router-WAN", "name": "R1"}
device.add {"model": "Router-WAN", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "s0/0/0", "b": "R2", "b_port": "s0/0/0", "cable": "serial-dce"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/0"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
ipv6config 2001:db8:1:1::10/64 2001:db8:1:1::1
```

```console PC2
ipconfig 192.168.2.10 255.255.255.0 192.168.2.1
ipv6config 2001:db8:1:2::10/64 2001:db8:1:2::1
```

## Part 1 — Interfaces

R1 holds the DCE end of the serial cable, so it sets the clock rate.

```console R1
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ip address 192.168.1.1 255.255.255.0
ipv6 address 2001:db8:1:1::1/64
no shutdown
interface g0/1
ip address 10.0.12.1 255.255.255.252
ipv6 address 2001:db8:12::1/64
no shutdown
interface s0/0/0
ip address 10.9.12.1 255.255.255.252
ipv6 address 2001:db8:912::1/64
clock rate 128000
no shutdown
end
```

```console R2
enable
configure terminal
ipv6 unicast-routing
interface g0/0
ip address 192.168.2.1 255.255.255.0
ipv6 address 2001:db8:1:2::1/64
no shutdown
interface g0/1
ip address 10.0.12.2 255.255.255.252
ipv6 address 2001:db8:12::2/64
no shutdown
interface s0/0/0
ip address 10.9.12.2 255.255.255.252
ipv6 address 2001:db8:912::2/64
no shutdown
end
```

## Part 2 — Static routes and floating static routes

R1 reaches PC2's LAN through the Ethernet link (distance 1, the default for static routes)
and, as a backup, through the serial link with distance 10. Only the best one is in the
table; the floating route waits.

```console R1
configure terminal
ip route 192.168.2.0 255.255.255.0 10.0.12.2
ip route 192.168.2.0 255.255.255.0 10.9.12.2 10
ipv6 route 2001:db8:1:2::/64 2001:db8:12::2
ipv6 route 2001:db8:1:2::/64 2001:db8:912::2 10
end
```

R2 is a stub: it sends everything it doesn't know towards R1, with its own floating
backups.

```console R2
configure terminal
ip route 0.0.0.0 0.0.0.0 10.0.12.1
ip route 0.0.0.0 0.0.0.0 10.9.12.1 10
ipv6 route ::/0 2001:db8:12::1
ipv6 route ::/0 2001:db8:912::1 10
end
```

```verify
R1# show ip route
  expect: 192.168.2.0/24       via 10.0.12.2 [1/0]
  reject: via 10.9.12.2
R1# show ipv6 route
  expect: 2001:db8:1:2::/64
  expect: 2001:db8:12::2
R2# show ip route
  expect: S*   0.0.0.0/0            via 10.0.12.1 [1/0]
ping PC1 192.168.2.10 => success
ping PC1 2001:db8:1:2::10 => success
PC1> tracert 192.168.2.10
  expect: 10.0.12.2
```

## Part 3 — The main link fails

```console R1
configure terminal
interface g0/1
shutdown
end
```

With G0/1 down, the routes through 10.0.12.2 are gone and the floating routes move in, on
both routers (R2 sees its G0/1 go down too):

```verify
R1# show ip route
  expect: 192.168.2.0/24       via 10.9.12.2 [10/0]
R1# show ipv6 route
  expect: 2001:db8:912::2
R2# show ip route
  expect: via 10.9.12.1 [10/0]
ping PC1 192.168.2.10 => success
ping PC1 2001:db8:1:2::10 => success
PC1> tracert 192.168.2.10
  expect: 10.9.12.2
```

Bring the link back and the main routes return:

```console R1
configure terminal
interface g0/1
no shutdown
end
```

```verify
R1# show ip route
  expect: via 10.0.12.2 [1/0]
  reject: via 10.9.12.2
ping PC1 192.168.2.10 => success
```

## Reflection

1. Why does the floating route need a distance higher than 1?
2. What would happen if the backup route on R1 had distance 1 too?
3. Why is a default route enough on R2 but not on R1?
