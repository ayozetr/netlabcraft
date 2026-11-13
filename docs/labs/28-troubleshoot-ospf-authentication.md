# Lab 28 — Troubleshooting: OSPF neighbours that disagree about authentication

**Course:** CCNA 3 (Enterprise Networking, Security, and Automation) · **Time:** 55 minutes

## Objectives

- Start from a triangle of routers that authenticate their OSPF packets with MD5 digests, then
  break each link in a different way: a mistyped key, a different key ID and a different
  authentication type.
- Tell the three apart from `show ip ospf neighbor`, `show ip ospf interface` and
  `debug ip ospf adj` first, and only then confirm in the configuration.
- Repair each one and prove the network converged, from the neighbour list, the route summary
  and the link-state database.

## Topology

```text
                       R1 ───────── R2
 PC1 ── G0/0 ─────────  │ G0/1    G0/1 │
                    G0/2 │              │ G0/2
                         │              │
 PC3 ── G0/0 ─────────  R3 ─────────────┘
                      G0/2    G0/1
```

| Device | Model | Interface | Address | Router ID |
|---|---|---|---|---|
| R1 | Router-3 | G0/0 | 192.168.1.1/24 | 1.1.1.1 |
| R1 | | G0/1 | 10.28.12.1/30 (to R2) | |
| R1 | | G0/2 | 10.28.13.1/30 (to R3) | |
| R2 | Router-3 | G0/1 | 10.28.12.2/30 (to R1) | 2.2.2.2 |
| R2 | | G0/2 | 10.28.23.1/30 (to R3) | |
| R3 | Router-3 | G0/0 | 192.168.3.1/24 | 3.3.3.3 |
| R3 | | G0/1 | 10.28.23.2/30 (to R2) | |
| R3 | | G0/2 | 10.28.13.2/30 (to R1) | |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw 192.168.1.1 | |
| PC3 | PC | Fa0 | 192.168.3.10/24, gw 192.168.3.1 | |

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-3", "name": "R2"}
device.add {"model": "Router-3", "name": "R3"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "R2", "a_port": "g0/2", "b": "R3", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/2", "b": "R3", "b_port": "g0/2"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC3", "b": "R3", "b_port": "g0/0"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC3
ipconfig 192.168.3.10 255.255.255.0 192.168.3.1
```

## Part 1 — The working network

OSPF process 1, everything in area 0. The area asks for message digests
(`area 0 authentication message-digest`), and every link between routers carries key 1
(`ip ospf message-digest-key 1 md5 Lab28Secret`). The LAN interfaces are passive: they are
advertised but form no neighbours, so they need no key.

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.28.12.1 255.255.255.252
ip ospf message-digest-key 1 md5 Lab28Secret
no shutdown
interface g0/2
ip address 10.28.13.1 255.255.255.252
ip ospf message-digest-key 1 md5 Lab28Secret
no shutdown
exit
router ospf 1
router-id 1.1.1.1
network 192.168.1.0 0.0.0.255 area 0
network 10.28.12.0 0.0.0.3 area 0
network 10.28.13.0 0.0.0.3 area 0
passive-interface g0/0
area 0 authentication message-digest
end
```

```console R2
enable
configure terminal
interface g0/1
ip address 10.28.12.2 255.255.255.252
ip ospf message-digest-key 1 md5 Lab28Secret
no shutdown
interface g0/2
ip address 10.28.23.1 255.255.255.252
ip ospf message-digest-key 1 md5 Lab28Secret
no shutdown
exit
router ospf 1
router-id 2.2.2.2
network 10.28.12.0 0.0.0.3 area 0
network 10.28.23.0 0.0.0.3 area 0
area 0 authentication message-digest
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 192.168.3.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.28.23.2 255.255.255.252
ip ospf message-digest-key 1 md5 Lab28Secret
no shutdown
interface g0/2
ip address 10.28.13.2 255.255.255.252
ip ospf message-digest-key 1 md5 Lab28Secret
no shutdown
exit
router ospf 1
router-id 3.3.3.3
network 192.168.3.0 0.0.0.255 area 0
network 10.28.23.0 0.0.0.3 area 0
network 10.28.13.0 0.0.0.3 area 0
passive-interface g0/0
area 0 authentication message-digest
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
R1# show ip ospf neighbor
  expect: 2.2.2.2
  expect: 3.3.3.3
  expect: FULL
R1# show ip ospf interface g0/1
  expect: Authentication: message digest, signing with key 1
R1# show running-config
  expect: area 0 authentication message-digest
  expect: ip ospf message-digest-key 1 md5
R1# show ip route ospf
  expect: 192.168.3.0/24
ping PC1 192.168.3.10 => success
```

Every OSPF packet on these links now has authentication type 2: the checksum field is zero, the
header holds the key ID and a sequence number, and a 16-byte MD5 digest of the packet and the
key follows it. Export a capture of one link (`netlabcraft-cli pcap`, or the capture panel) and
open a Hello in any packet viewer to see the type, the key ID and the digest.

## Part 2 — The faults

Three changes are made behind your back, one on each link.

```console R1
configure terminal
interface g0/1
ip ospf message-digest-key 1 md5 Lab28Secrt
end
```

```console R3
configure terminal
interface g0/1
no ip ospf message-digest-key 1
ip ospf message-digest-key 2 md5 Lab28Secret
exit
interface g0/2
ip ospf authentication
ip ospf authentication-key Lab28Pw
end
```

```lab
sim.advance {"seconds": 120}
```

## Part 3 — Diagnose

```verify
ping PC1 192.168.3.10 => failure
R1# show ip ospf neighbor
  reject: FULL
R1# show ip route ospf
  reject: 192.168.3.0
R3# show ip protocols summary
  expect: OSPF
```

Every OSPF link is down. All three have the same symptom, so look at each link's own
reason. `show ip ospf neighbor` lists the routers that share a link but are not neighbours,
and why:

```verify
R1# show ip ospf neighbor
  expect: Not a neighbour: 10.28.12.2
  expect: authentication fails
R2# show ip ospf neighbor
  expect: authentication fails
```

Ask for more on one interface at a time. `show ip ospf interface` prints the type each
interface signs with:

```verify
R1# show ip ospf interface g0/1
  expect: Authentication: message digest
R1# show ip ospf interface g0/2
  expect: Authentication: message digest
R3# show ip ospf interface g0/2
  expect: Authentication: clear-text password
R3# show ip ospf interface g0/1
  expect: signing with key 2
R2# show ip ospf interface g0/2
  expect: signing with key 1
```

The adjacency debug says in words which of the three it is. Turn it on at R1 and wait two
Hello intervals:

```console R1
debug ip ospf adj
```

```lab
sim.advance {"seconds": 30}
```

```verify
R1# show logging
  expect: stays Down
  expect: the message digest does not verify under key 1
  expect: the packet uses simple password authentication, the receiving interface expects message digest
R1# show debugging
  expect: OSPF adjacencies
```

### R1 and R2: a key that does not match

Both ends sign with key 1, but R1's key has a typo. The digest R2 sends does not verify under
R1's key, and the other way round. The digest is computed over the whole packet and the
padded key, so one wrong letter changes all of it. The symptom does not say which end has the
typo; the configuration does:

```verify
R1# show running-config
  expect: md5 Lab28Secrt
R2# show running-config
  expect: md5 Lab28Secret
```

Type the key again on R1:

```console R1
configure terminal
interface g0/1
ip ospf message-digest-key 1 md5 Lab28Secret
end
```

### R2 and R3: a different key ID

R3 signs with key 2, R2 only has key 1. The packet names the key it was signed with, and R2
has no key by that number. (`show ip ospf interface g0/1` on R3 gave it away: "signing with
key 2".) Put key 1 back on R3 and remove key 2:

```console R3
configure terminal
interface g0/1
no ip ospf message-digest-key 2
ip ospf message-digest-key 1 md5 Lab28Secret
end
```

### R1 and R3: a different type

R3's `ip ospf authentication` on G0/2 asks for a clear-text password. An interface's own
setting wins over the area's, so this link is the only one that does not use digests. The
two ends must use the same type before keys even matter. Remove the interface's setting, so
that the area's applies again, and the leftover password with it:

```console R3
configure terminal
interface g0/2
no ip ospf authentication
no ip ospf authentication-key
end
```

## Part 4 — Prove the repair

```console R1
undebug all
```

```lab
sim.advance {"seconds": 90}
```

```verify
R1# show ip ospf neighbor
  expect: 2.2.2.2
  expect: 3.3.3.3
  expect: FULL
  reject: authentication fails
R2# show ip ospf neighbor
  expect: 1.1.1.1
  expect: 3.3.3.3
R3# show ip ospf interface g0/2
  expect: Authentication: message digest, signing with key 1
R1# show ip protocols summary
  expect: OSPF
  expect: 1.1.1.1
R1# show ip route summary
  expect: OSPF
  expect: intra-area
R1# show ip ospf database router
  expect: Router 1.1.1.1
  expect: Router 2.2.2.2
  expect: Router 3.3.3.3
R1# show ip route 192.168.3.0 255.255.255.0
  expect: O
R3# show ip route ospf
  expect: 192.168.1.0/24
ping PC1 192.168.3.10 => success
ping PC3 192.168.1.10 => success
PC1> tracert 192.168.3.10
  expect: 192.168.1.1
  expect: 192.168.3.10
```

## Reflection

1. Which of the three faults would the adjacency debug have named without looking at the
   interface at all, and which one needed `show ip ospf interface`?
2. Why does a clear-text password protect less than a message digest, even though both
   stop a router with no key from joining?
3. The area asked for message digests and R3's interface asked for a password. Which won,
   and where did you see it?
4. After the repair, how many routers does the link-state database hold, and why is it the
   same on every router?
