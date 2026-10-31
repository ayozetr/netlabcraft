# Lab 15 — Standard and extended ACLs, numbered and named

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Write a numbered standard ACL and place it close to the destination.
- Write a named extended ACL that filters by protocol and port, and place it close to the
  source.
- Edit an ACL in place with sequence numbers: insert a line and remove one.
- Restrict remote logins to the router with an ACL on the VTY lines.
- Read the match counters to see which lines traffic hit.

## Topology

```text
 PC1, PC2 ── S1 ── (G0/0) R1 (G0/2) ── Server1 (web)
                          (G0/1)
                            │
                           PC3
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-3 | G0/0 | 192.168.10.1/24 (LAN A) |
| R1 | | G0/1 | 192.168.20.1/24 (LAN B) |
| R1 | | G0/2 | 172.16.1.1/24 (servers) |
| PC1 | PC | Fa0 | 192.168.10.10/24 |
| PC2 | PC | Fa0 | 192.168.10.20/24 |
| PC3 | PC | Fa0 | 192.168.20.10/24 |
| Server1 | Server | G0 | 172.16.1.10/24, web server on |

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
device.add {"model": "Server", "name": "Server1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "PC3", "b": "R1", "b_port": "g0/1"}
link.add {"a": "Server1", "b": "R1", "b_port": "g0/2"}
server.services {"device": "Server1", "services": {"http": {"on": true, "files": [{"name": "index.html", "content": "<h1>Intranet</h1>"}]}}}
```

```console PC1
ipconfig 192.168.10.10 255.255.255.0 192.168.10.1
```

```console PC2
ipconfig 192.168.10.20 255.255.255.0 192.168.10.1
```

```console PC3
ipconfig 192.168.20.10 255.255.255.0 192.168.20.1
```

```console Server1
ipconfig 172.16.1.10 255.255.255.0 172.16.1.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.10.1 255.255.255.0
no shutdown
interface g0/1
ip address 192.168.20.1 255.255.255.0
no shutdown
interface g0/2
ip address 172.16.1.1 255.255.255.0
no shutdown
end
```

```verify
ping PC2 172.16.1.10 => success
ping PC3 172.16.1.10 => success
```

## Part 1 — A standard ACL near the destination

Only PC1 from LAN A, and all of LAN B, may reach the server LAN. A standard ACL checks only
the source address, so it goes out of the interface facing the servers, where it can't get
in the way of anything else. Every ACL ends with an unwritten `deny any`.

```console R1
configure terminal
access-list 10 remark Who may reach the servers
access-list 10 permit host 192.168.10.10
access-list 10 permit 192.168.20.0 0.0.0.255
interface g0/2
ip access-group 10 out
end
```

```verify
ping PC1 172.16.1.10 => success
ping PC2 172.16.1.10 => failure
ping PC3 172.16.1.10 => success
ping PC2 192.168.20.10 => success
R1# show access-lists
  expect: Standard access list 10
  expect: 10 permit host 192.168.10.10 (7 matches)
  expect: (implicit) deny everything else (7 matches)
R1# show ip interface g0/2
  expect: Access list out: 10
```

PC2 still reaches LAN B: the ACL only filters what leaves towards the servers.

## Part 2 — A named extended ACL near the source

Take the standard ACL off. LAN B may now browse the web server but not ping it; everything
else stays open. An extended ACL can tell those apart, so it goes in on LAN B's interface,
where the traffic starts.

```console R1
configure terminal
interface g0/2
no ip access-group 10 out
exit
no access-list 10
ip access-list extended LAN-B-IN
permit tcp 192.168.20.0 0.0.0.255 host 172.16.1.10 eq 80
deny icmp 192.168.20.0 0.0.0.255 host 172.16.1.10
permit ip any any
exit
interface g0/1
ip access-group LAN-B-IN in
end
```

```verify
ping PC3 172.16.1.10 => failure
ping PC3 192.168.10.10 => success
run browser.get {"device": "PC3", "url": "http://172.16.1.10/index.html"}
  expect: Intranet
R1# show access-lists LAN-B-IN
  expect: Extended access list LAN-B-IN
  expect: 10 permit tcp 192.168.20.0 0.0.0.255 host 172.16.1.10 eq 80
  expect: 20 deny icmp
  expect: 30 permit ip any any
```

## Part 3 — Edit with sequence numbers

The help desk at PC3 must be able to ping the server. Lines are numbered 10, 20, 30; insert
line 15, before the `deny`, without retyping the list:

```console R1
configure terminal
ip access-list extended LAN-B-IN
15 permit icmp host 192.168.20.10 host 172.16.1.10
end
```

```verify
R1# show access-lists LAN-B-IN
  expect: 15 permit icmp host 192.168.20.10 host 172.16.1.10
ping PC3 172.16.1.10 => success
```

The exception is no longer wanted: remove line 15 only.

```console R1
configure terminal
ip access-list extended LAN-B-IN
no 15
end
```

```verify
R1# show access-lists LAN-B-IN
  reject: 15 permit
ping PC3 172.16.1.10 => failure
```

## Part 4 — Who may log in

Only PC1 may open a remote session to R1. A standard ACL on the VTY lines does it
(`access-class`):

```console R1
configure terminal
enable secret Class123
line vty 0 4
password Remote123
login
transport input telnet
access-class 20 in
exit
access-list 20 permit host 192.168.10.10
end
```

```verify
PC1> telnet 192.168.10.1
  expect: Open
PC1> Remote123
PC1> exit
PC2> telnet 192.168.10.1
  reject: Open
```

## Reflection

1. Why would the standard ACL of Part 1 be a bad idea on G0/0 inbound?
2. In Part 2, why is `permit ip any any` needed at the end?
3. What would the list look like after `ip access-list resequence LAN-B-IN 100 10`?
