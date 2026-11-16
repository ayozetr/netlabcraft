# Lab 47 — BGP session passwords and a conditional default route

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 45 minutes

## Objectives

- Protect a BGP session with a TCP MD5 password (`neighbor A.B.C.D password`) and say why a
  session whose two ends disagree never comes up.
- Read the symptom: a neighbour that stays in Active with a note about authentication, and
  no routes from it.
- Know what the console shows of a password: nothing in `show ip bgp neighbors`, and an obscured
  form in the running configuration once `service password-encryption` is on.
- Send a neighbour a default route with `neighbor A.B.C.D default-originate`, and make it
  conditional with a route map: the default is sent only while a route passes the map.

## Topology

```text
 R1 ───────────── R2 ───────────── R3
 AS 65001         AS 65002         AS 3000
 Lo1 172.16.1.1   10.47.12.0/24   10.47.23.0/24
```

| Device | Model | Interface | Address | Runs |
|---|---|---|---|---|
| R1 | Router-2 | G0/1 | 10.47.12.1/24 | BGP AS 65001 |
| R1 | | Loopback1 | 172.16.1.1/24 | |
| R2 | Router-2 | G0/0 | 10.47.12.2/24 | BGP AS 65002 |
| R2 | | G0/1 | 10.47.23.2/24 | |
| R3 | Router-2 | G0/0 | 10.47.23.3/24 | BGP AS 3000 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Router-2", "name": "R3"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/0"}
```

```console R1
enable
configure terminal
interface g0/1
ip address 10.47.12.1 255.255.255.0
no shutdown
interface loopback 1
ip address 172.16.1.1 255.255.255.0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.47.12.2 255.255.255.0
no shutdown
interface g0/1
ip address 10.47.23.2 255.255.255.0
no shutdown
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.47.23.3 255.255.255.0
no shutdown
end
```

## Part 1 — Two different passwords

A BGP session runs over a TCP connection. With `neighbor … password` every segment of that
connection carries an MD5 signature made from the password, and a segment with the wrong
signature is dropped without an answer. Both ends must therefore have the same password, or no
password at all. Give R1 and R2 a password each, and make a mistake on purpose: the two differ.
The passwords here are made up for the lab.

```console R1
configure terminal
router bgp 65001
bgp router-id 1.1.1.1
neighbor 10.47.12.2 remote-as 65002
neighbor 10.47.12.2 password lab-fake-key
network 172.16.1.0 mask 255.255.255.0
end
```

```console R2
configure terminal
router bgp 65002
bgp router-id 2.2.2.2
neighbor 10.47.12.1 remote-as 65001
neighbor 10.47.12.1 password lab-other-key
end
```

```lab
sim.advance {"seconds": 10}
```

The neighbours know each other's address and the AS numbers are right, yet nothing comes up. The
TCP connection never opens, so no OPEN message is ever exchanged: the state stays Active and the
neighbour view says why. R2 learns nothing from R1:

```verify
R2# show ip bgp neighbors 10.47.12.1
  expect: State: Active
  expect: TCP MD5 authentication failed
R1# show ip bgp neighbors 10.47.12.2
  expect: State: Active
  expect: TCP MD5 authentication failed
R2# show ip bgp
  reject: 172.16.1.0/24
```

## Part 2 — The same password

Give R2 the password R1 has. A session that was waiting starts again at once:

```console R2
configure terminal
router bgp 65002
neighbor 10.47.12.1 password lab-fake-key
end
```

```lab
sim.advance {"seconds": 10}
```

```verify
R2# show ip bgp neighbors 10.47.12.1
  expect: State: Established
  expect: MD5 password
  reject: lab-fake-key
R2# show ip bgp
  expect: 172.16.1.0/24
R1# show running-config
  expect: neighbor 10.47.12.2 password lab-fake-key
```

The neighbour view says a password is set and never prints it. The running configuration is
different: it has to be replayable, so it holds the password, in plain text, as a line password
does. `service password-encryption` obscures it the same way, and the obscured form typed back
gives the same password:

```console R1
configure terminal
service password-encryption
end
```

```verify
R1# show running-config
  expect: neighbor 10.47.12.2 password 7
  reject: lab-fake-key
```

A password has at most 25 characters. `no neighbor 10.47.12.2 password` takes it away, and a session
that is up keeps running until a segment fails, which on a quiet session means until its hold time
runs out. Peer groups carry a password to every member too (`neighbor NAME password …`), and
`show ip bgp peer-group` prints it as `******`.

## Part 3 — A default route towards R3

R3 is a customer that only needs a default route. Start the session between R2 and R3 (no
password on this one), then ask R2 to send R3 a default route whether or not it has one of its own:

```console R3
configure terminal
router bgp 3000
bgp router-id 3.3.3.3
neighbor 10.47.23.2 remote-as 65002
end
```

```console R2
configure terminal
router bgp 65002
neighbor 10.47.23.3 remote-as 3000
neighbor 10.47.23.3 default-originate
end
```

```lab
sim.advance {"seconds": 10}
```

```verify
R3# show ip bgp
  expect: 0.0.0.0/0
  expect: 172.16.1.0/24
R2# show ip bgp neighbors 10.47.23.3
  expect: Options: default-originate
```

## Part 4 — Only while a route is there

A default route that is sent no matter what can lead traffic into a black hole: if R2 loses
what it is the gateway to, R3 should stop using it. `default-originate route-map NAME` makes the
default conditional. R2 looks through its routes for one that the route map permits; while there
is one, it sends the default (with whatever the map sets, here one extra copy of its AS number),
and when there is none, it withdraws it. Watch for a prefix R2 does not have:

```console R2
configure terminal
ip prefix-list WATCH seq 5 permit 172.16.9.0/24
route-map DEF permit 10
match ip address prefix-list WATCH
set as-path prepend 65002
exit
router bgp 65002
neighbor 10.47.23.3 default-originate route-map DEF
end
```

```lab
sim.advance {"seconds": 10}
```

No route passes `WATCH`, so the default is withdrawn, while R1's network still reaches R3:

```verify
R3# show ip bgp
  reject: 0.0.0.0/0
  expect: 172.16.1.0/24
R2# show ip bgp neighbors 10.47.23.3
  expect: default-originate route-map DEF
R2# show running-config
  expect: neighbor 10.47.23.3 default-originate route-map DEF
```

Now watch for R1's network, which R2 does have:

```console R2
configure terminal
ip prefix-list WATCH seq 10 permit 172.16.1.0/24
end
```

```lab
sim.advance {"seconds": 10}
```

The default comes back, with the map's prepend in its AS path:

```verify
R3# show ip bgp
  expect: 0.0.0.0/0
  expect: 65002 65002 i
```

`no neighbor 10.47.23.3 default-originate route-map DEF` stops the default altogether, map
included.

```console R2
configure terminal
router bgp 65002
no neighbor 10.47.23.3 default-originate route-map DEF
end
```

```lab
sim.advance {"seconds": 10}
```

```verify
R3# show ip bgp
  reject: 0.0.0.0/0
R2# show running-config
  reject: default-originate
```

## What you learned

- `neighbor … password` protects a BGP session with TCP MD5: the two ends need the same
  password, or none. A mismatch keeps the session in Active with no OPEN exchanged, which looks
  like a routing problem until the neighbour view is read.
- The password is never shown by `show ip bgp neighbors`; the running configuration holds it,
  obscured when `service password-encryption` is on.
- `neighbor … default-originate` sends a default route whether or not there is one; with
  `route-map NAME` it is sent only while a route passes the map, and the map's `set` lines change
  it on its way out.
