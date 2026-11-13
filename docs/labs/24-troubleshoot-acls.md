# Lab 24 — Troubleshooting: an access list in the wrong place

**Course:** CCNA 3 (Enterprise Networking, Security, and Automation) · **Time:** 45 minutes

## Objectives

- Apply an extended ACL that lets one LAN browse a web server but nothing else reach it.
- Meet three faults: the ACL applied in the wrong direction, a stray entry at the top, and a
  wrong port number.
- Use `show access-lists` (match counters), `show ip interface` and `show running-config
  interface` to see what the ACL does, and edit it by sequence number.

## Topology

```text
 PC1 ── G0/0  R1  G0/1 ── Web1
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.24.1.1/24 |
| R1 | | G0/1 | 10.24.2.1/24 |
| PC1 | PC | Fa0 | 10.24.1.10/24, gw 10.24.1.1 |
| Web1 | Server | G0 | 10.24.2.10/24, gw 10.24.2.1 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "Server", "name": "Web1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "Web1", "b": "R1", "b_port": "g0/1"}
```

```console PC1
ipconfig 10.24.1.10 255.255.255.0 10.24.1.1
```

```console Web1
ipconfig 10.24.2.10 255.255.255.0 10.24.2.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.24.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.24.2.1 255.255.255.0
no shutdown
end
```

```verify
ping PC1 10.24.2.10 => success
run browser.get {"device": "PC1", "url": "http://10.24.2.10/index.html"}
  expect: 200
```

## Part 1 — The policy

PC1's LAN may open web pages on Web1 and do nothing else with it; everything else may go
anywhere. An extended ACL sees the protocol and the port, so it belongs on the interface where
the traffic enters the router, G0/0, in the inbound direction.

```console R1
configure terminal
ip access-list extended WEB-ONLY
10 permit tcp 10.24.1.0 0.0.0.255 host 10.24.2.10 eq 80
20 deny ip 10.24.1.0 0.0.0.255 host 10.24.2.10 log
30 permit ip any any
exit
interface g0/0
ip access-group WEB-ONLY in
end
```

```verify
ping PC1 10.24.2.10 => failure
ping PC1 10.24.1.1 => success
run browser.get {"device": "PC1", "url": "http://10.24.2.10/index.html"}
  expect: 200
R1# show access-lists WEB-ONLY
  expect: 10 permit tcp
  expect: 20 deny ip
  expect: log
R1# show ip interface g0/0
  expect: Access list in: WEB-ONLY
R1# show logging
  expect: access list WEB-ONLY line 20: denied icmp
```

The `log` on entry 20 makes each match leave a line in the log, so a deny can be seen as it
happens, not only counted. The counters can be reset to watch one test at a time:

```console R1
clear ip access-list counters WEB-ONLY
```

```verify
R1# show access-lists WEB-ONLY
  expect: 20 deny ip 10.24.1.0 0.0.0.255 host 10.24.2.10 log
  reject: matches)
```

## Part 2 — Fault 1: the right list, the wrong direction

```console R1
configure terminal
interface g0/0
no ip access-group WEB-ONLY in
ip access-group WEB-ONLY out
end
```

The ping to the server works again, which the policy forbids.

```verify
ping PC1 10.24.2.10 => success
R1# show ip interface g0/0
  expect: Access list in: none
  expect: Access list out: WEB-ONLY
R1# show running-config interface g0/0
  expect: ip access-group WEB-ONLY out
```

The ACL is on the interface, but outbound it only looks at what leaves towards PC1, not at what
PC1 sends. Look at the counters: no entry has been matched by PC1's traffic.

```verify
R1# show access-lists WEB-ONLY
  reject: 20 deny ip 10.24.1.0 0.0.0.255 host 10.24.2.10 (1
```

Take it off and apply it inbound:

```console R1
configure terminal
interface g0/0
no ip access-group WEB-ONLY out
ip access-group WEB-ONLY in
end
```

```verify
ping PC1 10.24.2.10 => failure
R1# show access-lists WEB-ONLY
  expect: 20 deny ip
```

## Part 3 — Fault 2: an entry at the top

Someone inserts an entry before the others to "unblock something for a minute":

```console R1
configure terminal
ip access-list extended WEB-ONLY
5 permit icmp any any
end
```

```verify
ping PC1 10.24.2.10 => success
R1# show access-lists WEB-ONLY
  expect: 5 permit icmp any any
  expect: 10 permit tcp
```

An ACL is read from the top and stops at the first match, so entry 5 permits every ping and
entries 10 and 20 are never reached. Remove it by its sequence number; the rest of the list stays.

```console R1
configure terminal
ip access-list extended WEB-ONLY
no 5
end
```

```verify
ping PC1 10.24.2.10 => failure
R1# show access-lists WEB-ONLY
  reject: 5 permit icmp any any
```

## Part 4 — Fault 3: the wrong port

```console R1
configure terminal
ip access-list extended WEB-ONLY
no 10
10 permit tcp 10.24.1.0 0.0.0.255 host 10.24.2.10 eq 8080
end
```

```verify
run browser.get {"device": "PC1", "url": "http://10.24.2.10/index.html"}
  reject: 200
R1# show access-lists WEB-ONLY
  expect: eq 8080
  expect: 20 deny ip
```

The web page no longer loads: HTTP uses port 80, the ACL permits 8080, so the request falls
through to the deny on entry 20, and its counter rises with each attempt. Put the right port back:

```console R1
configure terminal
ip access-list extended WEB-ONLY
no 10
10 permit tcp 10.24.1.0 0.0.0.255 host 10.24.2.10 eq 80
end
```

```verify
run browser.get {"device": "PC1", "url": "http://10.24.2.10/index.html"}
  expect: 200
ping PC1 10.24.2.10 => failure
ping PC1 10.24.1.1 => success
R1# show access-lists WEB-ONLY
  expect: 10 permit tcp 10.24.1.0 0.0.0.255 host 10.24.2.10 eq 80
```

## Reflection

1. Why does the ACL in Fault 1 still show on the interface and never match anything?
2. What would happen to PC1's other traffic if entry 30 were missing?
3. Why do replies from the server to PC1 not need an entry of their own here?
