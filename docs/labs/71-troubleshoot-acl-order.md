# Lab 71 — Troubleshooting: an ACL entry that can never match

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 40 minutes

## Objectives

- Read the match counters of `show access-lists` to find an entry that never matches because
  an entry above it already caught everything it was meant for.
- Fix the order without retyping the list: delete the entry that is in the wrong place and add
  it back at a sequence number below the others, then tidy the numbers with `resequence`.
- Limit `debug ip packet` to the access list under test to watch the packets of the one host
  that matters.

## Topology

```text
 PC1 ─┐
      ├── (G0/0) R1 (G0/1) ── Web1
 PC2 ─┘
   (through S1)
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.71.1.1/24 |
| R1 | | G0/1 | 10.71.2.1/24 |
| S1 | Switch-24 | | |
| PC1 | PC | Fa0 | 10.71.1.10/24, gateway 10.71.1.1 |
| PC2 | PC | Fa0 | 10.71.1.11/24, gateway 10.71.1.1 |
| Web1 | Server | G0 | 10.71.2.10/24, gateway 10.71.2.1 |

The policy: everyone on the LAN may use Web1 and everything else, except PC2, which may not open
its web pages. Pings to Web1 from PC2 stay allowed.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "Server", "name": "Web1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "fa0/24"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "Web1", "b": "R1", "b_port": "g0/1"}
```

```console PC1
ipconfig 10.71.1.10 255.255.255.0 10.71.1.1
```

```console PC2
ipconfig 10.71.1.11 255.255.255.0 10.71.1.1
```

```console Web1
ipconfig 10.71.2.10 255.255.255.0 10.71.2.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.71.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.71.2.1 255.255.255.0
no shutdown
end
```

```lab
sim.advance {"seconds": 35}
```

## Part 1 — The list as it was written

Someone wrote the general rule first, because it was the one they had in mind, and the
exception second.

```console R1
configure terminal
ip access-list extended LAN-IN
10 permit ip 10.71.1.0 0.0.0.255 any
20 deny tcp host 10.71.1.11 host 10.71.2.10 eq 80
exit
interface g0/0
ip access-group LAN-IN in
end
```

## Part 2 — The symptom

PC2 opens the web page it should not be allowed to open.

```verify
run browser.get {"device": "PC2", "url": "http://10.71.2.10/index.html"}
  expect: 200
run browser.get {"device": "PC1", "url": "http://10.71.2.10/index.html"}
  expect: 200
```

Both entries look right when read one at a time. The counters show how the router has used
them: an entry that has matched says how many times, and one that never matched says nothing.

```verify
R1# show access-lists LAN-IN
  expect: 10 permit ip 10.71.1.0 0.0.0.255 any (
  reject: 20 deny tcp host 10.71.1.11 host 10.71.2.10 eq 80 (
```

Entry 10 has all the matches and entry 20 has none: PC2's web requests are inside
10.71.1.0/24, and the list stops at the first entry that matches. Entry 20 is below an entry
that already permits everything it would deny. Watch one host's packets to be sure: limit
`debug ip packet` to an access list that names PC2.

```console R1
access-list 30 permit host 10.71.1.11
debug ip packet 30
clear logging
```

```verify
run browser.get {"device": "PC2", "url": "http://10.71.2.10/index.html"}
  expect: 200
R1# show logging
  expect: IP debug: 10.71.1.11 → 10.71.2.10, in GigabitEthernet0/0, routed out GigabitEthernet0/1
  reject: IP debug: 10.71.1.10 →
```

PC2's packets are routed out to Web1: the ACL did not stop them. Turn the debugging off.

```console R1
undebug all
```

## Part 3 — Put the exception first

A list is edited by sequence number. Remove the entry that is in the wrong place and add it
back below the other, then tidy up. Do not retype the list: retyping is how entries go missing.

```console R1
configure terminal
ip access-list extended LAN-IN
no 10
30 permit ip 10.71.1.0 0.0.0.255 any
exit
ip access-list resequence LAN-IN 10 10
end
clear ip access-list counters LAN-IN
```

```verify
R1# show access-lists LAN-IN
  expect: 10 deny tcp host 10.71.1.11 host 10.71.2.10 eq 80
  expect: 20 permit ip 10.71.1.0 0.0.0.255 any
  reject: 30 
run browser.get {"device": "PC2", "url": "http://10.71.2.10/index.html"}
  reject: 200
run browser.get {"device": "PC1", "url": "http://10.71.2.10/index.html"}
  expect: 200
ping PC2 10.71.2.10 => success
R1# show access-lists LAN-IN
  expect: 10 deny tcp host 10.71.1.11 host 10.71.2.10 eq 80 (
  expect: 20 permit ip 10.71.1.0 0.0.0.255 any (
```

Both entries match something now: the exception catches PC2's web requests and the general rule
takes everything else, including PC2's pings.

## Reflection

1. Why does a *deny* below a broad *permit* never match, but a *permit* below a broad *deny*
   fails in the same way?
2. What does an entry with no counter at all tell you, in a list that has been in use for a
   week?
3. Why is `no 10` followed by a new line at a *higher* number a better edit than retyping the whole list?
4. Where in this lab would a standard ACL (source address only) have been enough, and where not?
