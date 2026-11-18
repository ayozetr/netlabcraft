# Lab 78 — Policy-based routing: choosing the exit by who sends the packet

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 45 minutes

## Objectives

- Send the packets of one host out of a different interface than the routing table would, with
  a route map applied to the interface they arrive on.
- Read what the policy took with `show ip policy` and `show route-map`, and watch each decision
  with `debug ip policy`.
- See that the policy is asked before the routing table, and that a packet no clause matches (or
  whose next hop is unusable) is routed by the table as usual.

## Topology

```text
            ┌── R2 (10.78.2.2)   default route
 PC1 ── R1 ─┤
            └── R3 (10.78.3.2)   policy route
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-3 | G0/0 | 10.78.1.1/24 |
| R1 | | G0/1 | 10.78.2.1/24 |
| R1 | | G0/2 | 10.78.3.1/24 |
| R2 | Router-2 | G0/0 | 10.78.2.2/24 |
| R3 | Router-2 | G0/0 | 10.78.3.2/24 |
| PC1 | PC | Fa0 | 10.78.1.10/24, gateway 10.78.1.1 |

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
device.add {"model": "PC", "name": "PC1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R1", "a_port": "g0/2", "b": "R3", "b_port": "g0/0"}
```

```console PC1
ipconfig 10.78.1.10 255.255.255.0 10.78.1.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.78.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.78.2.1 255.255.255.0
no shutdown
interface g0/2
ip address 10.78.3.1 255.255.255.0
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 10.78.2.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.78.2.2 255.255.255.0
no shutdown
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.78.3.2 255.255.255.0
no shutdown
end
```

```lab
sim.advance {"seconds": 5}
```

## Part 1 — What the table does

Without a policy, everything leaves through the default route, towards R2. PC1 sends to an
address nobody answers for; what matters is where R1 sends it, which its ARP table shows.

```verify
ping PC1 198.51.100.7 => failure
R1# show arp
  expect: 10.78.2.2
  reject: 10.78.3.2
```

## Part 2 — A policy for PC1

An access list names the traffic, a route map says where it goes, and the interface the traffic
arrives on applies the route map.

```console R1
configure terminal
access-list 110 permit icmp host 10.78.1.10 any
route-map VIA-R3 permit 10
match ip address 110
set ip next-hop 10.78.3.2
exit
interface g0/0
ip policy route-map VIA-R3
end
clear arp-cache
debug ip policy
```

```verify
R1# show ip policy
  expect: GigabitEthernet0/0
  expect: VIA-R3
ping PC1 198.51.100.7 => failure
R1# show logging
  expect: route map VIA-R3 sequence 10 matched: next hop 10.78.3.2 out GigabitEthernet0/2
R1# show arp
  expect: 10.78.3.2
R1# show route-map VIA-R3
  expect: policy routing took
  expect: match access list: 110
```

PC1's echo requests went to R3 although the table says R2: the policy is asked first. Nothing
else changed: R1's default route is still towards R2.

```verify
R1# show ip route
  expect: Default route: via 10.78.2.2
```

## Part 3 — Packets the policy is not asked about

The policy is applied to the packets that arrive on G0/0. A packet R1 sends itself does not
arrive on any interface, so the route map does not see it (`ip local policy route-map` would
apply one): R1's own ping goes by the table.

```console R1
clear logging
clear arp-cache
```

```verify
ping R1 198.51.100.7 => failure
R1# show logging
  reject: route map VIA-R3 sequence 10 matched
R1# show arp
  expect: 10.78.2.2
  reject: 10.78.3.2
```

## Part 4 — Take the policy off

```console R1
undebug all
configure terminal
interface g0/0
no ip policy route-map
end
clear arp-cache
```

```verify
ping PC1 198.51.100.7 => failure
R1# show arp
  expect: 10.78.2.2
  reject: 10.78.3.2
```

## Reflection

1. Why does the policy decide where the packet goes even though the table has a perfectly good
   route to the destination?
2. What happens to a packet that matches no clause of the route map? And one that matches a
   `permit` clause whose next hop is not on a connected network?
3. Why is the policy applied on the interface the packets *arrive* on, and what would you use for the
   packets R1 itself sends?
4. How does `set ip default next-hop` differ from `set ip next-hop`?
