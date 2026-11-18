# Lab 65 — Reading `debug` output: ICMP, ARP and routed packets

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Turn on `debug ip icmp`, `debug ip packet` and `debug arp` on one router, read what they
  print into `show logging`, and turn them off again (`undebug all`).
- Find a missing return route from the output: the request is routed on, the reply is
  dropped with "no route".
- Find a wrong default gateway on a host from an ARP question nobody answers.
- Narrow `debug ip packet` to one access list so a busy router prints only what matters.

## Topology

```text
 PC1 ── (G0/0) R1 (G0/1) ──── (G0/0) R2 (G0/1) ── PC2
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| R1 | | G0/1 | 10.65.0.1/30 |
| R2 | Router-2 | G0/0 | 10.65.0.2/30 |
| R2 | | G0/1 | 192.168.2.1/24 |
| PC1 | PC | Fa0 | 192.168.1.10/24, gateway 192.168.1.1 |
| PC2 | PC | Fa0 | 192.168.2.10/24, gateway 192.168.2.1 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/1"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC2
ipconfig 192.168.2.10 255.255.255.0 192.168.2.1
```

R1 knows the way to PC2's network. R2 was set up by someone who stopped one line early.

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.65.0.1 255.255.255.252
no shutdown
exit
ip route 192.168.2.0 255.255.255.0 10.65.0.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.65.0.2 255.255.255.252
no shutdown
interface g0/1
ip address 192.168.2.1 255.255.255.0
no shutdown
end
```

## Part 1 — The symptom

```verify
ping PC1 192.168.2.10 => failure
ping R1 10.65.0.2 => success
```

R1 reaches R2 across their shared link, and PC1 does not reach PC2. The next
question is whether the request reaches R2 and whether R2 sends it on. `show` commands describe
the state; `debug` commands describe what happens to the next packets. Turn on two on R2.

```console R2
debug ip icmp
debug ip packet
```

```verify
R2# show debugging
  expect: ICMP
  expect: IP packets
```

Send a ping from PC1, then read the log of R2.

```verify
ping PC1 192.168.2.10 => failure
R2# show logging
  expect: IP debug: 192.168.1.10 → 192.168.2.10, in GigabitEthernet0/0, routed out GigabitEthernet0/1
  expect: IP debug: 192.168.2.10 → 192.168.1.10, in GigabitEthernet0/1, no route: dropped
```

The first line is the echo request being routed on towards PC2. PC2 answers; the answer arrives
at R2 on its way back, and the second line says what became of it: R2 has no route to
192.168.1.0/24, so the reply is dropped there. Each packet has one line, and the line says where
it came in and what the router did with it. `debug ip icmp` agrees from the other side: pinging
the router itself shows the request arriving and no way back.

```verify
ping PC1 192.168.2.1 => failure
R2# show logging
  expect: ICMP debug: echo request from 192.168.1.10 to 192.168.2.1
```

```console R2
undebug all
configure terminal
ip route 192.168.1.0 255.255.255.0 10.65.0.1
end
clear logging
debug ip packet
```

Now both directions are routed:

```verify
ping PC1 192.168.2.10 => success
R2# show logging
  expect: IP debug: 192.168.1.10 → 192.168.2.10, in GigabitEthernet0/0, routed out GigabitEthernet0/1
  expect: IP debug: 192.168.2.10 → 192.168.1.10, in GigabitEthernet0/1, routed out GigabitEthernet0/0 via 10.65.0.1
  reject: no route: dropped
```

## Part 2 — Limit what is printed with an access list

On a busy router `debug ip packet` prints every packet. Name the packets you care about with an
access list, and only those are reported:

```console R2
undebug all
configure terminal
access-list 20 permit host 192.168.2.10
end
clear logging
debug ip packet 20
```

```verify
R2# show debugging
  expect: list 20
ping PC1 192.168.2.10 => success
R2# show logging
  expect: IP debug: 192.168.2.10 → 192.168.1.10
  reject: IP debug: 192.168.1.10 → 192.168.2.10
```

Only the packets that PC2 sent are reported: the list matches the source address. A list that
does not exist is refused, and so is a typing mistake:

```verify
R2# debug ip packet 99
  expect: There is no access list
```

## Part 3 — ARP: a host with the wrong gateway

PC2's gateway is mistyped (192.168.2.254; the router's address is .1). Local traffic still works,
because ARP finds hosts on the same network. Traffic for anything else needs the gateway's MAC
address, and nobody has 192.168.2.254.

```console R2
undebug all
debug arp
clear logging
```

```console PC2
ipconfig 192.168.2.10 255.255.255.0 192.168.2.254
```

```verify
ping PC2 192.168.1.10 => failure
R2# show logging
  expect: ARP debug: who has 192.168.2.254? 192.168.2.10
  reject: ARP debug: 192.168.2.254 is at
```

The ARP request is a question to the whole network and it is printed; an answer would be
printed as `192.168.2.254 is at …`, and there is none. Fix the gateway:

```console PC2
ipconfig 192.168.2.10 255.255.255.0 192.168.2.1
```

```verify
ping PC2 192.168.1.10 => success
```

Always finish by turning the debugging off, so the log does not fill up with traffic:

```console R2
undebug all
```

```verify
R2# show debugging
  expect: No debugging is on
```

## Reflection

1. Why does the router print a line for the dropped reply, and which other line would you look for
   if it printed none?
2. `debug ip packet` prints a line for each packet. What would limit it on a busy router, other
   than an access list?
3. The ARP question for 192.168.2.254 is sent every time PC2 wants to leave its network. Where
   would you see that on the wire?
4. Why is it a good habit to type `undebug all` when you are done?
