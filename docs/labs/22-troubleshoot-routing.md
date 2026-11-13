# Lab 22 — Troubleshooting: static routes and host settings

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Build a two-router network with static routes and show that it works.
- Break it with three faults — a wrong next hop, a missing route and a wrong subnet mask on a
  host — and find each with `ping`, `traceroute`, `show ip route` and `ipconfig`.
- Repair each one and prove it.

## Topology

```text
 PC1 ── G0/0  R1  G0/1 ──────── G0/1  R2  G0/0 ── PC2
                   10.22.0.0/30
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| R1 | | G0/1 | 10.22.0.1/30 |
| R2 | Router-2 | G0/0 | 192.168.2.1/24 |
| R2 | | G0/1 | 10.22.0.2/30 |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw 192.168.1.1 |
| PC2 | PC | Fa0 | 192.168.2.10/24, gw 192.168.2.1 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/0"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC2
ipconfig 192.168.2.10 255.255.255.0 192.168.2.1
```

## Part 1 — The working network

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.22.0.1 255.255.255.252
no shutdown
exit
ip route 192.168.2.0 255.255.255.0 10.22.0.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 192.168.2.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.22.0.2 255.255.255.252
no shutdown
exit
ip route 192.168.1.0 255.255.255.0 10.22.0.1
end
```

```verify
ping PC1 192.168.2.10 => success
ping PC2 192.168.1.10 => success
```

## Part 2 — The faults

Three changes are made while you are away. Type them as they are.

```console R2
configure terminal
no ip route 192.168.1.0 255.255.255.0 10.22.0.1
ip route 192.168.1.0 255.255.255.0 10.22.0.5
end
```

```console R1
configure terminal
no ip route 192.168.2.0 255.255.255.0 10.22.0.2
end
```

## Part 3 — Diagnose and repair, one symptom at a time

### Symptom 1: PC1 cannot reach PC2

```verify
ping PC1 192.168.2.10 => failure
PC1> tracert 192.168.2.10
  expect: 192.168.1.1
R1# show ip route
  expect: 10.22.0.0/30
  reject: 192.168.2.0
R1# show ip route 192.168.2.10
  expect: not in the routing table
```

The trace stops at R1, the PC's gateway, and R1 has no route for 192.168.2.0/24. Add it back.

```console R1
configure terminal
ip route 192.168.2.0 255.255.255.0 10.22.0.2
end
```

```verify
R1# show ip route static
  expect: 192.168.2.0/24
ping PC1 192.168.2.10 => failure
```

### Symptom 2: the request now leaves, but there is no answer

R1 sends the packets to R2 now, and the pings from the PC still fail. Ask R1 to ping with its
own address on the link, then look at R2.

```verify
R1# ping 192.168.2.10
  expect: 100% answered
R2# show ip route
  expect: 192.168.2.0/24
  reject: 192.168.1.0
R2# show running-config | include ip route
  expect: ip route 192.168.1.0 255.255.255.0 10.22.0.5
```

R1's own ping works: the reply goes to 10.22.0.1, which R2 reaches over the connected link. A
reply to 192.168.1.10 needs a route, and R2 has none in its table: the route is configured, but
its next hop 10.22.0.5 is not on any connected network, so the router cannot use it and leaves it
out. The link is a /30, so the only other address on it is 10.22.0.1.

```console R2
configure terminal
no ip route 192.168.1.0 255.255.255.0 10.22.0.5
ip route 192.168.1.0 255.255.255.0 10.22.0.1
end
```

```verify
R2# show ip route static
  expect: 192.168.1.0/24
ping PC1 192.168.2.10 => success
ping PC2 192.168.1.10 => success
PC1> tracert 192.168.2.10
  expect: 192.168.1.1
  expect: 10.22.0.2
  expect: 192.168.2.10
```

### Symptom 3: PC1 reaches its gateway but nothing beyond

A technician "fixed" PC1 by hand:

```console PC1
ipconfig 192.168.1.10 255.255.0.0 192.168.1.1
```

With a /16 mask, 192.168.2.10 looks like a neighbour, so PC1 does not send it to the gateway: it
ARPs for it directly. Routers answer such an ARP on behalf of the host they can route to (proxy
ARP, on by default), so this fault can hide for a long time:

```verify
PC1> ipconfig
  expect: 255.255.0.0
ping PC1 192.168.2.10 => success
PC1> arp -a
  expect: 192.168.2.10
R1# show ip interface g0/0
  expect: Proxy ARP: on
```

The ARP table gives it away: the address of a host on another subnet is there, with the
router's MAC address. Turn proxy ARP off on R1's LAN interface and the fault shows itself:

```console R1
configure terminal
interface g0/0
no ip proxy-arp
end
```

```console PC1
arp -d
```

```verify
R1# show ip interface g0/0
  expect: Proxy ARP: off
R1# show running-config interface g0/0
  expect: no ip proxy-arp
ping PC1 192.168.1.1 => success
ping PC1 192.168.2.10 => failure
```

Now repair the real cause, the mask:

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
arp -d
```

```verify
ping PC1 192.168.2.10 => success
ping PC2 192.168.1.10 => success
```

## Reflection

1. Why did the trace show R1 and then stop, instead of showing a timeout at R2?
2. A route with a next hop that is not on a connected network: what would a router with a recursive lookup do?
3. Which of the three faults would a routing protocol have repaired by itself?
