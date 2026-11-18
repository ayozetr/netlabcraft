# Lab 51 — OSPF flooding, retransmission and the life of an LSA

**Course:** CCNA 3 follow-up (CCNP-style routing) · **Time:** 40 minutes

## Objectives

- See the link-state update and acknowledgement packets that carry an LSA from one router to the
  next, and the neighbour's request list and retransmission list in `show ip ospf neighbor detail`.
- Set the retransmit interval of an interface (`ip ospf retransmit-interval`) and read what
  `debug ip ospf retransmission` and `debug ip ospf flood` report.
- Follow an LSA's age and sequence number over simulated time: the router that originated it
  sends it again every 30 minutes with the next sequence number, and the age starts over.

## Topology

```text
 R1 ───────────── R2
   10.51.12.0/30
```

| Device | Model | Interface | Address | Router ID |
|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.51.12.1/30 | 1.1.1.1 |
| R2 | Router-2 | G0/0 | 10.51.12.2/30 | 2.2.2.2 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
link.add {"a": "R1", "a_port": "g0/0", "b": "R2", "b_port": "g0/0"}
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.51.12.1 255.255.255.252
ip ospf network point-to-point
no shutdown
exit
router ospf 1
router-id 1.1.1.1
network 10.51.12.0 0.0.0.3 area 0
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.51.12.2 255.255.255.252
ip ospf network point-to-point
no shutdown
exit
router ospf 1
router-id 2.2.2.2
network 10.51.12.0 0.0.0.3 area 0
end
```

```lab
sim.advance {"seconds": 60}
```

## Part 1 — The neighbour's lists

Every LSA an update carries stays on the sender's retransmission list until the neighbour
acknowledges it. Once the databases are the same both lists are empty:

```verify
R1# show ip ospf neighbor detail
  expect: state FULL
  expect: in the request list
  expect: in the retransmission list
R1# show ip ospf database router
  expect: 0x80000001
```

## Part 2 — The retransmit interval

An update that is not acknowledged is sent again after the retransmit interval (5 seconds
unless it is set). Lengthen it on R1's interface and ask for the reports:

```console R1
configure terminal
interface g0/0
ip ospf retransmit-interval 12
end
debug ip ospf retransmission
debug ip ospf flood
```

```verify
R1# show running-config
  expect: ip ospf retransmit-interval 12
R1# show debugging
  expect: OSPF retransmissions
  expect: OSPF flooding
```

## Part 3 — A change is flooded at once

A new cost changes R1's router LSA. R1 originates it again with the next sequence number,
floods it and R2 acknowledges it:

```console R1
configure terminal
interface g0/0
ip ospf cost 10
end
```

```lab
sim.advance {"seconds": 5}
```

```verify
R1# show ip ospf database router self-originate
  expect: 0x80000002
R2# show ip ospf database router 1.1.1.1
  expect: 0x80000002
R1# show logging
  expect: acknowledged
```

## Part 4 — Half an hour later

Nothing has changed, yet the originator sends its LSAs again every 1800 seconds (LSRefreshTime).
The sequence number goes up and the age starts over; an LSA nobody refreshes would reach
MaxAge (3600 seconds) and be removed:

```lab
sim.advance {"seconds": 1900}
```

```verify
R1# show ip ospf database router self-originate
  expect: 0x80000003
R2# show ip ospf neighbor
  expect: FULL
```

## Reflection

1. Why does an acknowledgement name the LSA's sequence number and checksum?
2. What would the neighbour list of R1 show if R2 never acknowledged an update?
3. Why does a router refresh an LSA before it reaches MaxAge?
