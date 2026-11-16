# Lab 46 — Route tags and BGP peer groups

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 75 minutes

## Objectives

- Label a static route with a tag (`ip route … tag N`) and read the tag in the routing table, with
  `show ip route tag N` and in the entry views.
- Follow a tag through redistribution: into OSPF's external LSA, and out of a route map that
  matches on it (`match tag`) or changes it (`set tag`).
- Give every redistributed route one tag with `redistribute … tag N`, and say which setting wins
  when several name a tag.
- Put the settings several BGP neighbours share in a peer group (`neighbor NAME peer-group`) and
  join neighbours to it (`neighbor A.B.C.D peer-group NAME`).
- Say what a member takes from its group, what it keeps of its own, and what happens when the
  group changes.
- Read the group in `show ip bgp peer-group`, `show ip bgp neighbors` and the running
  configuration.

## Topology

```text
 R1 ───────────── R2 ───────────── R3
 Lo1 172.16.1.1   10.46.12.0/24    10.46.23.0/24   Lo3 172.16.3.1
```

| Device | Model | Interface | Address | Runs |
|---|---|---|---|---|
| R1 | Router-2 | G0/1 | 10.46.12.1/24 | BGP AS 65001 |
| R1 | | Loopback1 | 172.16.1.1/24 | |
| R2 | Router-2 | G0/0 | 10.46.12.2/24 | BGP AS 65002 |
| R2 | | G0/1 | 10.46.23.2/24 | OSPF 1, area 0 |
| R3 | Router-2 | G0/0 | 10.46.23.3/24 | OSPF 1, area 0, BGP AS 65001 |
| R3 | | Loopback3 | 172.16.3.1/24 | |

R2 is the router in the middle: it owns three static routes that point at R1 (and carry tags),
hands them to R3 through OSPF in part 2, and in part 5 it is the provider with two customer
routers, R1 and R3, which share one AS number.

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
ip address 10.46.12.1 255.255.255.0
no shutdown
interface loopback 1
ip address 172.16.1.1 255.255.255.0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.46.12.2 255.255.255.0
no shutdown
interface g0/1
ip address 10.46.23.2 255.255.255.0
no shutdown
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.46.23.3 255.255.255.0
no shutdown
interface loopback 3
ip address 172.16.3.1 255.255.255.0
end
```

## Part 1 — A tag on a static route

A route tag is a number a route carries with it. It changes nothing about where traffic goes; it
is a label that route maps and redistribution can read later. A static route gets one with `tag`
at the end of the line, after the distance if there is one. Give R2 three routes, two of them
tagged:

```console R2
configure terminal
ip route 10.50.0.0 255.255.255.0 10.46.12.1 tag 7
ip route 10.60.0.0 255.255.255.0 10.46.12.1 tag 8
ip route 10.70.0.0 255.255.255.0 10.46.12.1
end
```

The routing table shows the tag at the end of the row. `show ip route tag 7` lists only the routes
that carry that tag, and the entry views say it too. A route with no tag shows none:

```verify
R2# show ip route
  expect: 10.50.0.0/24
  expect: tag 7
  expect: tag 8
R2# show ip route tag 7
  expect: 10.50.0.0/24
  reject: 10.60.0.0/24
R2# show ip route tag 99
  expect: No route carries tag 99
R2# show ip route 10.50.0.0 255.255.255.0
  expect: Tag 7
R2# show ip route 10.70.0.0 255.255.255.0
  reject: Tag
R2# show running-config
  expect: ip route 10.50.0.0 255.255.255.0 10.46.12.1 tag 7
```

Typing a route again with another tag replaces the old one, and the tag has to be a number from 1
up (a route with no tag has tag 0):

```console R2
configure terminal
ip route 10.50.0.0 255.255.255.0 10.46.12.1 tag 9
ip route 10.50.0.0 255.255.255.0 10.46.12.1 tag 7
end
```

```verify
R2# show running-config
  expect: ip route 10.50.0.0 255.255.255.0 10.46.12.1 tag 7
  reject: tag 9
```

## Part 2 — The tag goes into OSPF's external LSA

R2 redistributes its static routes into OSPF. With no tag on the `redistribute` line and no route
map, each route keeps the tag it has, and OSPF carries it in the AS-external LSA:

```console R2
configure terminal
router ospf 1
network 10.46.23.0 0.0.0.255 area 0
redistribute static subnets
end
```

```console R3
configure terminal
router ospf 1
network 10.46.23.0 0.0.0.255 area 0
end
```

```lab
sim.advance {"seconds": 30}
```

R3 learns the three networks as external routes with the tags they had on R2. The database entry
names the tag too:

```verify
R3# show ip route
  expect: O E2 10.50.0.0/24
  expect: tag 7
  expect: tag 8
R3# show ip route tag 8
  expect: 10.60.0.0/24
  reject: 10.50.0.0/24
R3# show ip route 10.50.0.9
  expect: Tag 7
R3# show ip ospf database external 10.50.0.0
  expect: tag 7
R3# show ip ospf database external 10.70.0.0
  expect: tag 0
```

## Part 3 — Choose by tag, and set a new one

A route map can match the tag of the routes it is given (`match tag`) and give the ones it lets
through another (`set tag`). A route no clause permits is dropped. Let only the routes tagged 7
into OSPF, and tag them 77:

```console R2
configure terminal
route-map ONLY7 permit 10
match tag 7
set tag 77
exit
router ospf 1
redistribute static subnets route-map ONLY7
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R2# show route-map ONLY7
  expect: match tag: 7
  expect: set tag 77
R2# show running-config
  expect: match tag 7
  expect: set tag 77
  expect: redistribute static subnets route-map ONLY7
R3# show ip route
  expect: 10.50.0.0/24
  expect: tag 77
  reject: 10.60.0.0/24
  reject: 10.70.0.0/24
```

## Part 4 — One tag for everything a line redistributes

`redistribute … tag N` under OSPF gives every route the line takes the same tag. A `set tag` in a
route map named on the same line comes first; here there is no map, so the line's tag is used:

```console R2
configure terminal
router ospf 1
redistribute static metric 5 metric-type 1 tag 100 subnets
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
R3# show ip route
  expect: O E1 10.50.0.0/24
  expect: O E1 10.70.0.0/24
  expect: tag 100
  reject: tag 7
  reject: tag 77
R3# show ip ospf database external 10.60.0.0
  expect: tag 100
R2# show running-config
  expect: redistribute static metric 5 metric-type 1 tag 100 subnets
```

## Part 5 — A BGP peer group

R2 is a provider with two customer routers. R1 and R3 both sit in AS 65001 (two sites of one
customer), and R2 wants the same settings on both sessions: the remote AS, communities sent, and a
prefix list that lets only the customer's `172.16.x.0` networks in. Typed neighbour by neighbour
that is the same three lines twice, and an edit has to be made twice. A peer group holds them once.

Start the customers first:

```console R1
configure terminal
router bgp 65001
bgp router-id 1.1.1.1
neighbor 10.46.12.2 remote-as 65002
network 172.16.1.0 mask 255.255.255.0
end
```

```console R3
configure terminal
router bgp 65001
bgp router-id 3.3.3.3
neighbor 10.46.23.2 remote-as 65002
network 172.16.3.0 mask 255.255.255.0
end
```

Now the group on R2. `neighbor CUST peer-group` creates it, the lines that name `CUST` set its
settings, and `neighbor A.B.C.D peer-group CUST` joins a neighbour, adding it if it was not there.
A member that has not been given a remote AS of its own takes the group's:

```console R2
configure terminal
ip prefix-list SITES permit 172.16.0.0/16 le 24
router bgp 65002
bgp router-id 2.2.2.2
neighbor CUST peer-group
neighbor CUST remote-as 65001
neighbor CUST send-community
neighbor CUST prefix-list SITES in
neighbor 10.46.12.1 peer-group CUST
neighbor 10.46.23.3 peer-group CUST
end
```

```lab
sim.advance {"seconds": 10}
```

Both sessions come up with nothing typed under the neighbours themselves:

```verify
R2# show ip bgp
  expect: 172.16.1.0/24
  expect: 172.16.3.0/24
R2# show ip bgp summary
  expect: 10.46.12.1
  expect: 10.46.23.3
R2# show ip bgp peer-group
  expect: BGP peer group CUST
  expect: remote-as 65001
  expect: send-community
  expect: prefix-list SITES in
  expect: Members: 2
  expect: 10.46.12.1 Established
  expect: 10.46.23.3 Established
R2# show ip bgp neighbors 10.46.12.1
  expect: Member of peer group CUST
  expect: Options: send-community
  expect: Policy: inbound prefix list SITES
```

The running configuration writes the group once and each member as one line. What a member takes
from the group is not repeated under it:

```verify
R2# show running-config
  expect: neighbor CUST peer-group
  expect: neighbor CUST remote-as 65001
  expect: neighbor 10.46.12.1 peer-group CUST
  expect: neighbor 10.46.23.3 peer-group CUST
  reject: neighbor 10.46.12.1 remote-as
  reject: neighbor 10.46.23.3 send-community
```

## Part 6 — A member's own setting, and a change to the group

A member can set a value of its own for anything the group sets. It keeps it when the group
changes, and `show ip bgp peer-group` lists it next to the member. R3's customer is allowed to
send nothing but a network that does not exist, to see what happens to its routes:

```console R2
configure terminal
ip prefix-list ELSEWHERE permit 192.0.2.0/24
router bgp 65002
neighbor 10.46.23.3 prefix-list ELSEWHERE in
end
```

```verify
R2# show ip bgp
  expect: 172.16.1.0/24
  reject: 172.16.3.0/24
R2# show ip bgp peer-group CUST
  expect: 10.46.23.3 Established (its own: prefix-list ELSEWHERE in)
R2# show ip bgp neighbors 10.46.23.3
  expect: Policy: inbound prefix list ELSEWHERE
R2# show running-config
  expect: neighbor 10.46.23.3 prefix-list ELSEWHERE in
```

Taking the member's own line off gives it the group's again, and the group's list applies to both
members. Changing the group's list reaches every member that was following it:

```console R2
configure terminal
router bgp 65002
no neighbor 10.46.23.3 prefix-list ELSEWHERE in
end
```

```verify
R2# show ip bgp
  expect: 172.16.3.0/24
```

```console R2
configure terminal
router bgp 65002
neighbor CUST prefix-list ELSEWHERE in
end
```

```verify
R2# show ip bgp
  reject: 172.16.1.0/24
  reject: 172.16.3.0/24
R2# show ip bgp neighbors 10.46.12.1
  expect: Policy: inbound prefix list ELSEWHERE
R2# show ip bgp neighbors 10.46.23.3
  expect: Policy: inbound prefix list ELSEWHERE
```

A neighbour leaves its group with `no neighbor A.B.C.D peer-group CUST`; what it only had from the
group goes with it, so a neighbour with no remote AS of its own has no session. Deleting the group
(`no neighbor CUST peer-group`) takes every member out of it the same way:

```console R2
configure terminal
router bgp 65002
no neighbor 10.46.12.1 peer-group CUST
end
```

```verify
R2# show ip bgp peer-group CUST
  expect: Members: 1
R2# show running-config
  reject: neighbor 10.46.12.1 peer-group CUST
  expect: neighbor 10.46.23.3 peer-group CUST
```

## What you learned

- A route tag is a label a route carries: `ip route … tag N` sets it, the routing table and
  `show ip route tag N` show it, and OSPF's external LSA, EIGRP's external routes and RIP's route
  tag field carry it to the next router.
- `match tag` and `set tag` in a route map, and `redistribute … tag N` under OSPF, are how a tag
  is read and changed on the way from one protocol to another.
- A BGP peer group keeps the settings neighbours share in one place. A member takes what it has
  not set itself, keeps what it has, and follows the group when the group changes.
