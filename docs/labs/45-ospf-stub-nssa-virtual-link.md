# Lab 45 — OSPF stub, NSSA and virtual-link areas

**Course:** CCNA 3 follow-up (Enterprise Networking, Security and Automation) · **Time:** 75 minutes

## Objectives

- Turn an area into a stub area and a totally stubby area, and read what the area no longer
  receives (type 5 LSAs, then type 3 LSAs) and what it gets in their place (a default route).
- See why both ends of a link must agree on the area type, and read the reason when they do not.
- Turn an area into an NSSA: read the type 7 LSA, find the border router that translates it
  into a type 5 LSA, and tell `O N2` from `O E2` in the routing table.
- Join an area that does not touch the backbone with a virtual link.
- Read the sequence number and the age of an LSA, and see what makes a router originate it again.

## Topology

```text
                          area 0                      area 1                     area 2
 PC1 ── G0/1 R1 G0/0 ──────────── G0/0 R2 G0/1 ──────────── G0/0 R3 G0/1 ──────────── G0/0 R4 G0/1 ── PC4
                                       G0/2
                                         │ area 3
                                       G0/0
 PC5 ── G0/1 R5
```

| Device | Model | Interface | Address | Area | Router ID |
|---|---|---|---|---|---|
| R1 | Router-3 | G0/0 | 10.45.12.1/24 | 0 | 1.1.1.1 |
| R1 | | G0/1 | 192.168.1.1/24 | 0 | |
| R2 | Router-3 | G0/0 | 10.45.12.2/24 | 0 | 2.2.2.2 |
| R2 | | G0/1 | 10.45.23.2/24 | 1 | |
| R2 | | G0/2 | 10.45.25.2/24 | 3 | |
| R3 | Router-3 | G0/0 | 10.45.23.3/24 | 1 | 3.3.3.3 |
| R3 | | G0/1 | 10.45.34.3/24 | 2 | |
| R4 | Router-2 | G0/0 | 10.45.34.4/24 | 2 | 4.4.4.4 |
| R4 | | G0/1 | 192.168.4.1/24 | 2 | |
| R5 | Router-2 | G0/0 | 10.45.25.5/24 | 3 | 5.5.5.5 |
| R5 | | G0/1 | 192.168.5.1/24 | 3 | |
| PC1 | PC | Fa0 | 192.168.1.10/24, gw .1 | | |
| PC4 | PC | Fa0 | 192.168.4.10/24, gw .1 | | |
| PC5 | PC | Fa0 | 192.168.5.10/24, gw .1 | | |

R2 is the border router of areas 1 and 3, R3 of areas 1 and 2. R1 has a static route to
172.16.0.0/16, which it redistributes into OSPF; R5 will get one to 172.20.0.0/16 in part 3.
Area 2 is joined to the rest of the network only through area 1, not through area 0: that is
the fault part 4 repairs.

```lab
device.add {"model": "Router-3", "name": "R1"}
device.add {"model": "Router-3", "name": "R2"}
device.add {"model": "Router-3", "name": "R3"}
device.add {"model": "Router-2", "name": "R4"}
device.add {"model": "Router-2", "name": "R5"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC4"}
device.add {"model": "PC", "name": "PC5"}
link.add {"a": "R1", "a_port": "g0/0", "b": "R2", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/0"}
link.add {"a": "R3", "a_port": "g0/1", "b": "R4", "b_port": "g0/0"}
link.add {"a": "R2", "a_port": "g0/2", "b": "R5", "b_port": "g0/0"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/1"}
link.add {"a": "PC4", "b": "R4", "b_port": "g0/1"}
link.add {"a": "PC5", "b": "R5", "b_port": "g0/1"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC4
ipconfig 192.168.4.10 255.255.255.0 192.168.4.1
```

```console PC5
ipconfig 192.168.5.10 255.255.255.0 192.168.5.1
```

## Part 1 — Build the five areas

```console R1
enable
configure terminal
interface g0/0
ip address 10.45.12.1 255.255.255.0
no shutdown
interface g0/1
ip address 192.168.1.1 255.255.255.0
no shutdown
exit
ip route 172.16.0.0 255.255.0.0 10.45.12.99
router ospf 1
router-id 1.1.1.1
network 10.45.12.0 0.0.0.255 area 0
network 192.168.1.0 0.0.0.255 area 0
passive-interface g0/1
redistribute static subnets
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.45.12.2 255.255.255.0
no shutdown
interface g0/1
ip address 10.45.23.2 255.255.255.0
no shutdown
interface g0/2
ip address 10.45.25.2 255.255.255.0
no shutdown
exit
router ospf 1
router-id 2.2.2.2
network 10.45.12.0 0.0.0.255 area 0
network 10.45.23.0 0.0.0.255 area 1
network 10.45.25.0 0.0.0.255 area 3
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.45.23.3 255.255.255.0
no shutdown
interface g0/1
ip address 10.45.34.3 255.255.255.0
no shutdown
exit
router ospf 1
router-id 3.3.3.3
network 10.45.23.0 0.0.0.255 area 1
network 10.45.34.0 0.0.0.255 area 2
end
```

```console R4
enable
configure terminal
interface g0/0
ip address 10.45.34.4 255.255.255.0
no shutdown
interface g0/1
ip address 192.168.4.1 255.255.255.0
no shutdown
exit
router ospf 1
router-id 4.4.4.4
network 10.45.34.0 0.0.0.255 area 2
network 192.168.4.0 0.0.0.255 area 2
passive-interface g0/1
end
```

```console R5
enable
configure terminal
interface g0/0
ip address 10.45.25.5 255.255.255.0
no shutdown
interface g0/1
ip address 192.168.5.1 255.255.255.0
no shutdown
exit
router ospf 1
router-id 5.5.5.5
network 10.45.25.0 0.0.0.255 area 3
network 192.168.5.0 0.0.0.255 area 3
passive-interface g0/1
end
```

```lab
sim.advance {"seconds": 60}
```

R5 is in area 3 with nothing special configured, so it hears everything: the networks of the
other areas as inter-area routes and R1's static route as an external type 2 route.

```verify
ping PC1 192.168.5.10 => success
R5# show ip route
  expect: O IA 192.168.1.0/24
  expect: O E2 172.16.0.0/16
R5# show ip ospf database external
  expect: Network 172.16.0.0/16
  expect: advertised by 1.1.1.1
```

Area 2 is another matter. It touches area 1 only, and every route between two areas has to go
through the backbone, area 0. The border router R3 is not attached to it, so area 2's networks
are not advertised anywhere else and area 2 hears nothing from outside:

```verify
ping PC1 192.168.4.10 => failure
R1# show ip route
  reject: 192.168.4.0
R4# show ip route
  reject: 192.168.1.0
  reject: 10.45.12.0
R3# show ip route
  expect: O IA 192.168.1.0/24
```

## Part 2 — A stub area and a totally stubby area

Area 3 only has one way out, through R2, so R5 does not need to know every network of the
company. Make it a stub area. First on R2 alone:

```console R2
configure terminal
router ospf 1
area 3 stub
end
```

```lab
sim.advance {"seconds": 30}
```

The hello packets carry the area type in their options (the E bit is set in a normal area and
clear in a stub area), and the two routers must agree. They do not, so there is no adjacency, and
both routers say why:

```verify
R5# show ip ospf neighbor
  expect: No OSPF neighbours
  expect: the area is a normal area here and a stub area there
R2# show ip ospf neighbor
  expect: 1.1.1.1
  expect: 3.3.3.3
  expect: the area is a stub area here and a normal area there
  expect: E bit 0, N bit 0 here; E bit 1, N bit 0 there
```

Make R5 a stub too:

```console R5
configure terminal
router ospf 1
area 3 stub
end
```

```lab
sim.advance {"seconds": 30}
```

The external route is gone, and in its place the border router sends a default route as an
inter-area route (`O*IA`). The area's other routes stay:

```verify
ping PC5 192.168.1.10 => success
R5# show ip ospf neighbor
  expect: 2.2.2.2
R5# show ip route
  expect: O*IA 0.0.0.0/0
  expect: O IA 192.168.1.0/24
  reject: 172.16.0.0
R5# show ip ospf database external
  expect: No external LSAs
R2# show ip ospf database external
  expect: Network 172.16.0.0/16
```

R5 holds no type 5 LSAs at all (a stub area has none), while R2, which also sits in area 0,
keeps them for its backbone side. On the border router, `show ip ospf` names the area type:

```verify
R2# show ip ospf
  expect: Area 3: stub area
```

Now make the area totally stubby: the border router stops sending the summaries of the other
areas as well, and the default route is all R5 has left. A default route's cost can be set with
`area 3 default-cost`:

```console R2
configure terminal
router ospf 1
area 3 stub no-summary
area 3 default-cost 5
end
```

```verify
ping PC5 192.168.1.10 => success
R5# show ip route
  expect: O*IA 0.0.0.0/0
  expect: [110/6]
  reject: 192.168.1.0
R5# show ip ospf database summary
  expect: Network 0.0.0.0/0
  expect: advertised by 2.2.2.2, metric 5
  reject: 192.168.1.0
R2# show running-config
  expect: area 3 stub no-summary
  expect: area 3 default-cost 5
```

The cost of the route is the cost of the way to R2 (1) plus the default cost (5).

## Part 3 — A not-so-stubby area

R5 now has a static route to a network of its own, 172.20.0.0/16, that it has to hand to the
rest of the network. A stub area cannot do that: it has no type 5 LSAs, and an external route is
one. An NSSA can: its routers originate a **type 7** LSA, which stays inside the area, and the
border router translates it into a type 5 LSA for everyone else. Change area 3 on both ends
(taking the stub settings off first) and redistribute:

```console R2
configure terminal
router ospf 1
no area 3 stub
area 3 nssa
end
```

```console R5
configure terminal
ip route 172.20.0.0 255.255.0.0 192.168.5.99
router ospf 1
no area 3 stub
area 3 nssa
redistribute static subnets
end
```

```lab
sim.advance {"seconds": 60}
```

The type 7 LSA is in area 3, and says who will translate it:

```verify
R5# show ip ospf database nssa-external
  expect: Type 7 external link states, area 3
  expect: Network 172.20.0.0/16
  expect: advertised by 5.5.5.5, type 2 metric 20
  expect: 2.2.2.2 translates it into a type 5 LSA
R2# show ip ospf
  expect: Area 3: NSSA
  expect: translator: this router
R5# show ip ospf
  expect: translator: router 2.2.2.2
```

When an NSSA has several border routers, the one with the highest router ID translates. Outside
the area the route is an ordinary type 5 LSA, advertised by R2, not by R5, and the other
routers call it external; R2 itself, in the NSSA, calls it `N2`:

```verify
R1# show ip ospf database external
  expect: Network 172.20.0.0/16
  expect: advertised by 2.2.2.2
R1# show ip ospf database nssa-external
  expect: No NSSA external LSAs
R1# show ip route
  expect: O E2 172.20.0.0/16
R2# show ip route
  expect: O N2 172.20.0.0/16
ping PC1 192.168.5.10 => success
```

An NSSA still gets no type 5 LSAs from outside, so R1's own external route does not come in:

```verify
R5# show ip route
  expect: O IA 192.168.1.0/24
  reject: 172.16.0.0
R5# show ip ospf database external
  expect: No external LSAs
```

An NSSA has no default route unless the border router is asked for one. With
`default-information-originate` it sends a type 7 default, an external route of type 2 (`O*N2`):

```console R2
configure terminal
router ospf 1
area 3 nssa default-information-originate
end
```

```verify
R5# show ip route
  expect: O*N2 0.0.0.0/0
R5# show ip ospf database nssa-external
  expect: Network 0.0.0.0/0
  expect: not translated
R2# show running-config
  expect: area 3 nssa default-information-originate
```

The default route is not translated: it stays in the area. Finally the totally stubby form,
`no-summary`, which keeps the other areas' routes out and replaces the default route with a
type 3 one (so it is `O*IA` again). R5's own route still goes out:

```console R2
configure terminal
router ospf 1
area 3 nssa no-summary
end
```

```verify
ping PC5 192.168.1.10 => success
R5# show ip route
  expect: O*IA 0.0.0.0/0
  reject: O*N2 0.0.0.0/0
  reject: 192.168.1.0
R1# show ip route
  expect: O E2 172.20.0.0/16
```

An area is a stub area or an NSSA, not both, and the backbone is neither:

```verify
R2# configure terminal
R2# router ospf 1
R2# area 3 stub
  expect: is an NSSA
R2# area 0 nssa
  expect: The backbone
R2# end
```

## Part 4 — Join area 2 to the backbone with a virtual link

Every area must touch area 0. Area 2 does not, but it does touch area 1, and area 1 touches
area 0 at R2. A **virtual link** is a backbone adjacency carried across a normal area between two
border routers: R3 joins the backbone through R2, across area 1. Configure one end only
first (it names the other end by its router ID):

```console R2
configure terminal
router ospf 1
area 1 virtual-link 3.3.3.3
end
```

```verify
R2# show ip ospf virtual-links
  expect: to router 3.3.3.3
  expect: is down
  expect: has no `area 1 virtual-link 2.2.2.2` line
```

Both ends have to name each other:

```console R3
configure terminal
router ospf 1
area 1 virtual-link 2.2.2.2
end
```

```lab
sim.advance {"seconds": 60}
```

```verify
R2# show ip ospf virtual-links
  expect: to router 3.3.3.3
  expect: is up
  expect: Transit area 1, cost 1 across it
R3# show ip ospf
  expect: 1 virtual link configured, 1 up
```

R3 is now attached to the backbone, so it can pass routes between area 2 and the others:

```verify
ping PC1 192.168.4.10 => success
R1# show ip route
  expect: O IA 192.168.4.0/24
  expect: O IA 10.45.34.0/24
R4# show ip route
  expect: O IA 192.168.1.0/24
```

The transit area cannot be the backbone, a stub area or an NSSA:

```verify
R2# configure terminal
R2# router ospf 1
R2# area 0 virtual-link 3.3.3.3
  expect: The backbone
R2# end
```

## Part 5 — Sequence numbers and ages

Each LSA has a sequence number, starting at 0x80000001 when its router first originates it, and
an age in seconds that grows as time passes. A router originates its LSA again, with the next
number and age 0, when something in it changes, and anyway every 30 minutes (refresh). An LSA
that reaches 3600 seconds is removed from the database, which only happens to LSAs whose router
has stopped refreshing them. R1's own router LSA:

```verify
R1# show ip ospf database router self-originate
  expect: Router 1.1.1.1, sequence 0x800000
  expect: 2 links
R1# show ip ospf database
  expect: Age
  expect: Seq#
```

Shut R1's LAN interface: its router LSA loses the stub network, so R1 originates it again, and
the age starts over (nothing has passed since):

```console R1
configure terminal
interface g0/1
shutdown
end
```

```verify
R1# show ip ospf database router self-originate
  expect: 1 link, age 0 s
```

The new number is larger than the one above; R2 holds the same copy, with the same number.
Restore the interface (another origination), then clear the OSPF process: everything starts again
at 0x80000001.

```console R1
configure terminal
interface g0/1
no shutdown
end
```

```verify
R1# clear ip ospf process
  expect: restarted
R1# show ip ospf database router self-originate
  expect: sequence 0x80000001
  expect: 2 links
```

## Reflection

1. Why did R5 lose its neighbour R2 when only R2 was made a stub area, and which bit of the
   hello says so?
2. A stub area has no type 5 LSAs. Where did R5's route to 172.16.0.0/16 go, and how does R5 still
   reach it?
3. Compare `show ip route` on R1 and R2 for 172.20.0.0/16. Why is it `O E2` on one and `O N2` on
   the other, and which LSA does each learn it from?
4. Why did the virtual link need a line on both routers, and what would break it afterwards?
5. After the process restart, R1's router LSA is at sequence 0x80000001 again. What was it before,
   and which events raised it?
