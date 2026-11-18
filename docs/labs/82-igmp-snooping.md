# Lab 82 — IGMP snooping: multicast only where it was asked for

**Course:** CCNA 2 follow-up (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- See a switch flood multicast to every port, then stop that with `ip igmp snooping`.
- Join and leave a multicast group from a PC (`igmp join`, `igmp leave`) and read the groups the switch
  learned with `show ip igmp snooping groups`.
- Turn on the querier, so the switch asks for members every minute, and see a second switch find the port
  that leads to the querier (`show ip igmp snooping mrouter`).

## Topology

```text
 PC1 ──Fa0/1┐
 PC2 ──Fa0/2├─ S1 ──Gi0/1────Gi0/1── S2
 PC3 ──Fa0/3┘
```

| Device | Model | Address |
|---|---|---|
| PC1 | PC | 10.0.0.1/24 |
| PC2 | PC | 10.0.0.2/24 |
| PC3 | PC (the sender) | 10.0.0.3/24 |
| S1 | Switch-24 | the querier, source 10.0.0.250 |
| S2 | Switch-24 | learns the router port |

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "PC1", "a_port": "fa0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "a_port": "fa0", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "PC3", "a_port": "fa0", "b": "S1", "b_port": "fa0/3"}
link.add {"a": "S1", "a_port": "g0/1", "b": "S2", "b_port": "g0/1"}
```

```console PC1
ipconfig 10.0.0.1 255.255.255.0
```

```console PC2
ipconfig 10.0.0.2 255.255.255.0
```

```console PC3
ipconfig 10.0.0.3 255.255.255.0
```

## Part 1 — Without snooping

PC1 joins the group 239.1.1.1. PC3 pings the group: every member answers. A switch that does not
look at IGMP sends the multicast out of every port, so PC2 receives frames it never asked for.

```console PC1
igmp join 239.1.1.1
```

```console PC3
ping -n 1 239.1.1.1
```

```verify
PC3> ping -n 1 239.1.1.1
  expect: reply from 10.0.0.1
```

## Part 2 — With snooping

```console S1
enable
configure terminal
interface g0/1
switchport mode trunk
exit
ip igmp snooping
end
```

The switch learns from the reports it hears, so PC1 reports again.

```console PC1
igmp join 239.1.1.1
```

```verify
S1# show ip igmp snooping groups
  expect: 239.1.1.1
  expect: Fa0/1
S1# show ip igmp snooping
  expect: IGMP snooping:  enabled
```

PC3 pings again: PC1 answers, and PC2 sees nothing, because Fa0/2 is not a member port of the
group. Join PC2 and the switch adds its port; leave, and it takes it out.

```console PC2
igmp join 239.1.1.1
```

```verify
S1# show ip igmp snooping groups
  expect: Fa0/2
```

```console PC1
igmp leave 239.1.1.1
```

```console PC2
igmp leave 239.1.1.1
```

```verify
S1# show ip igmp snooping groups
  expect: No groups
PC3> ping -n 1 239.1.1.1
  expect: no answer
```

## Part 3 — A querier and a router port

Hosts leave the network without saying so, and a switch that only waits for reports forgets a
member after 260 seconds. A querier asks every minute, and the members answer.

```console S1
configure terminal
ip igmp snooping querier
ip igmp snooping querier address 10.0.0.250
end
```

```console S2
enable
configure terminal
interface g0/1
switchport mode trunk
exit
ip igmp snooping
end
```

```console PC1
igmp join 239.1.1.1
```

```lab
sim.advance {"seconds": 130}
```

```verify
S1# show ip igmp snooping querier
  expect: 10.0.0.250
S2# show ip igmp snooping mrouter
  expect: Gi0/1
```

S2 heard the queries on Gi0/1 and now treats that port as the way to the multicast router, so
multicast reaches it even when nobody behind S2 has joined.

## Reflection

1. Why does a switch send reports only to the router ports, and not to every member?
2. What happens to multicast in a VLAN where snooping is off (`no ip igmp snooping vlan N`)?
3. Which frames does a snooping switch still flood, and why (think of `224.0.0.5`)?
4. What would a host that left without sending a leave message cost the network?
