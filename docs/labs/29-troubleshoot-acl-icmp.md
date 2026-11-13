# Lab 29 — Troubleshooting: an ACL that breaks traceroute but not ping

**Course:** CCNA 3 (Enterprise Networking, Security, and Automation) · **Time:** 45 minutes

## Objectives

- Meet an access list that drops "most ICMP" and see what it keeps: `ping` works and
  `traceroute` does not.
- Name ICMP messages in an ACL by type and code (`11 0`, `3 4`) and by their names
  (`echo-reply`, `ttl-exceeded`, `packet-too-big`).
- Read the match counters of each line, fix the list by sequence number, and see that a line
  inserted above the others does not move their counters.

## Topology

```text
 PC1 ── G0/0  R1  G0/1 ──── G0/0  R2  G0/1 ── Web1
                10.29.12.0/30
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.29.1.1/24 |
| R1 | | G0/1 | 10.29.12.1/30 |
| R2 | Router-2 | G0/0 | 10.29.12.2/30 |
| R2 | | G0/1 | 10.29.2.1/24 |
| PC1 | PC | Fa0 | 10.29.1.10/24, gw 10.29.1.1 |
| Web1 | Server | G0 | 10.29.2.10/24, gw 10.29.2.1 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "Server", "name": "Web1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "Web1", "b": "R2", "b_port": "g0/1"}
```

```console PC1
ipconfig 10.29.1.10 255.255.255.0 10.29.1.1
```

```console Web1
ipconfig 10.29.2.10 255.255.255.0 10.29.2.1
```

## Part 1 — The working network

```console R1
enable
configure terminal
interface g0/0
ip address 10.29.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.29.12.1 255.255.255.252
no shutdown
exit
ip route 10.29.2.0 255.255.255.0 10.29.12.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.29.12.2 255.255.255.252
no shutdown
interface g0/1
ip address 10.29.2.1 255.255.255.0
no shutdown
exit
ip route 10.29.1.0 255.255.255.0 10.29.12.1
end
```

```verify
ping PC1 10.29.2.10 => success
PC1> tracert 10.29.2.10
  expect: 10.29.1.1
  expect: 10.29.12.2
  expect: 10.29.2.10
```

`tracert` sends probes with a TTL of 1, 2, 3. Each router that drops one answers with an ICMP
"time exceeded" (type 11, code 0), and that is how the hops are named. The destination answers
the last probe with an echo reply (type 0).

## Part 2 — The fault

The security team's rule is "nothing from the core network may talk ICMP to the LAN except
replies to our own pings". They apply it on the way into R1 from R2:

```console R1
configure terminal
ip access-list extended FROM-CORE
permit tcp any any established
permit icmp any any echo-reply
deny icmp any any log
permit ip any any
exit
interface g0/1
ip access-group FROM-CORE in
end
```

## Part 3 — Diagnose

Ping still works, and so do the web server's TCP answers. Traceroute does not:

```verify
ping PC1 10.29.2.10 => success
PC1> tracert 10.29.2.10
  reject: 10.29.12.2
  expect: 10.29.2.10
```

The trace names the first router and the web server, and nothing between: R2's "time exceeded"
answers never reach the PC. R1 drops them as they come in on G0/1. The match counters say which
line did it:

```verify
R1# show access-lists FROM-CORE
  expect: 10 permit tcp any any established
  expect: 20 permit icmp any any echo-reply (
  expect: 30 deny icmp any any log (
  expect: (implicit) deny everything else
R1# show ip interface g0/1
  expect: FROM-CORE
R1# show logging
  expect: denied icmp
```

The deny line has matches (the dropped "time exceeded" messages) and the echo-reply line has
matches too (every ping). Both are in the log, because the deny line has `log`.

### First repair: let TTL-expired messages through

Insert a line above the deny, by sequence number, naming the message by its type and code
(11 is "time exceeded", code 0 is "TTL expired in transit"). NetLabCraft prints a type and code
that have a name by the name:

```console R1
configure terminal
ip access-list extended FROM-CORE
25 permit icmp any any 11 0
end
```

```verify
R1# show access-lists FROM-CORE
  expect: 25 permit icmp any any ttl-exceeded
  expect: 30 deny icmp any any log (
```

The new line is above the deny, so what it permits no longer reaches it. The lines below it keep
their own counters; the counters follow their line, not its position in the list. Run the trace
again:

```verify
PC1> tracert 10.29.2.10
  expect: 10.29.12.2
  expect: 10.29.2.10
R1# show access-lists FROM-CORE
  expect: 25 permit icmp any any ttl-exceeded (
  expect: 20 permit icmp any any echo-reply (
```

### Second repair: the message that makes large downloads work

A rule that drops all other ICMP also drops "fragmentation needed" (type 3, code 4), the answer
a router gives when a packet does not fit the next link and has the Don't Fragment bit set.
Without it a sender never learns to use smaller packets (path MTU discovery), and big transfers
hang while small ones work. Allow that one message by its type and code:

```console R1
configure terminal
ip access-list extended FROM-CORE
27 permit icmp any any 3 4
end
```

```verify
R1# show access-lists FROM-CORE
  expect: 27 permit icmp any any packet-too-big
```

The line is named by what it stands for, whether you typed `3 4` or `packet-too-big`.

### A line typed twice

Somebody pastes the echo-reply rule again, with no sequence number:

```console R1
configure terminal
ip access-list extended FROM-CORE
```

```verify
R1# permit icmp any any echo-reply
  expect: Line 20 already holds this rule
R1# do show access-lists FROM-CORE
  expect: 20 permit icmp any any echo-reply
```

```console R1
end
```

NetLabCraft does not add the same rule twice when it is typed without a number: it says which
line already has it (`Line 20 already holds this rule`). With a sequence number of its own, a
rule can be repeated; that second line would count on its own and, below the first, never
match anything.

## Part 4 — Prove the repair

```verify
ping PC1 10.29.2.10 => success
PC1> tracert 10.29.2.10
  expect: 10.29.1.1
  expect: 10.29.12.2
  expect: 10.29.2.10
R1# show running-config
  expect: permit icmp any any ttl-exceeded
  expect: permit icmp any any packet-too-big
  expect: ip access-group FROM-CORE in
R1# show access-lists FROM-CORE
  expect: (implicit) deny everything else
```

The list is still tight: anything else ICMP (a redirect, an unreachable message other than
"fragmentation needed") still matches `30 deny icmp any any`.

## Reflection

1. Why does `ping` succeed while the ACL denies "most ICMP"? Which line lets it through?
2. A trace shows `*` at hop 2 and the destination at hop 3. Where were the "time exceeded"
   messages dropped, and which command proves it?
3. What is the difference between `unreachable` and `packet-too-big` in an ACL line, and which
   one does path MTU discovery need?
4. You inserted line 25. Why did lines 20 and 30 keep their counters?
