# Lab 77 — IP SLA and a tracked default route that fails over

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 45 minutes

## Objectives

- Measure a path with an IP SLA ICMP echo operation and read its results.
- Follow the operation with a tracked object and tie a static default route to it.
- Keep a floating static route as the backup and watch the default move when the primary
  provider stops answering, and come back when it does.

## Topology

```text
            ┌── ISP1 (10.77.1.2)
 R1 ── S1 ──┤
            └── ISP2 (10.77.1.3)
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 10.77.1.1/24 |
| ISP1 | Router-2 | G0/0 | 10.77.1.2/24 |
| ISP2 | Router-2 | G0/0 | 10.77.1.3/24 |
| S1 | Switch-24 | | |

R1 reaches both providers through the switch, so when ISP1's port goes down R1's own port
stays up: only the probes can tell that the provider is gone.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "ISP1"}
device.add {"model": "Router-2", "name": "ISP2"}
device.add {"model": "Switch-24", "name": "S1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "fa0/24"}
link.add {"a": "ISP1", "a_port": "g0/0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "ISP2", "a_port": "g0/0", "b": "S1", "b_port": "fa0/2"}
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.77.1.1 255.255.255.0
no shutdown
end
```

```console ISP1
enable
configure terminal
interface g0/0
ip address 10.77.1.2 255.255.255.0
no shutdown
end
```

```console ISP2
enable
configure terminal
interface g0/0
ip address 10.77.1.3 255.255.255.0
no shutdown
end
```

```lab
sim.advance {"seconds": 35}
```

## Part 1 — Probe the primary provider

An IP SLA operation sends a probe on a schedule. Create operation 1: an ICMP echo to ISP1
every 5 seconds, giving each probe two seconds to be answered. Nothing is sent until the
operation is scheduled.

```console R1
configure terminal
ip sla 1
icmp-echo 10.77.1.2
frequency 5
timeout 2000
exit
end
```

```verify
R1# show ip sla configuration 1
  expect: Test: icmp-echo 10.77.1.2
  expect: Frequency 5 s, timeout 2000 ms
  expect: not scheduled
R1# show ip sla statistics 1
  expect: no probe has run yet
```

Start it now and let it run for the rest of the lab.

```console R1
configure terminal
ip sla schedule 1 life forever start-time now
end
```

```lab
sim.advance {"seconds": 12}
```

```verify
R1# show ip sla statistics 1
  expect: Latest outcome: OK
  expect: answered, 0 over threshold, 0 failed
R1# show ip sla summary
  expect: icmp-echo
  expect: running
```

## Part 2 — Follow the operation and tie the default route to it

A tracked object says "up" while the operation's last probe was answered. The primary default
route follows it; the backup has a higher administrative distance, so it only carries traffic
while the primary is not in the table.

```console R1
configure terminal
track 1 ip sla 1 reachability
exit
ip route 0.0.0.0 0.0.0.0 10.77.1.2 track 1
ip route 0.0.0.0 0.0.0.0 10.77.1.3 200
end
```

```verify
R1# show track 1
  expect: ip sla 1 reachability
  expect: State is Up
  expect: static route 0.0.0.0/0 10.77.1.2
R1# show ip route
  expect: Default route: via 10.77.1.2
R1# show running-config
  expect: ip route 0.0.0.0 0.0.0.0 10.77.1.2 track 1
  expect: ip route 0.0.0.0 0.0.0.0 10.77.1.3 200
```

## Part 3 — The provider goes away

ISP1's port goes down. R1's own port does not (the switch keeps it up), so the interface
state cannot tell R1 anything. The probes can: after the next probe times out, the object goes
down, the primary default leaves the table and the floating route takes over.

```console ISP1
configure terminal
interface g0/0
shutdown
end
```

```lab
sim.advance {"seconds": 12}
```

```verify
R1# show ip sla statistics 1
  expect: Latest outcome: failed
R1# show track 1
  expect: State is Down
R1# show ip route
  expect: Default route: via 10.77.1.3
  reject: Default route: via 10.77.1.2
```

## Part 4 — The provider comes back

```console ISP1
configure terminal
interface g0/0
no shutdown
end
```

The switch port towards ISP1 needs its spanning-tree delay before it forwards again, so the
first probes after the link returns still fail; give it time, then let the operation probe once more.

```lab
sim.advance {"seconds": 40}
sim.advance {"seconds": 12}
```

```verify
R1# show ip sla statistics 1
  expect: Latest outcome: OK
R1# show track 1
  expect: State is Up
R1# show ip route
  expect: Default route: via 10.77.1.2
```

The default route returned to the primary provider by itself, because the floating route has a
distance of 200 and the tracked one has the default of 1.

## Reflection

1. Why did the interface of R1 never go down, and what would a route that follows
   `track interface g0/0 line-protocol` have done in Part 3?
2. How long does R1 keep using a dead provider in the worst case, given `frequency 5` and
   `timeout 2000`? What would you change to shorten it, and what does that cost?
3. What would `delay down 10 up 30` on the tracked object change, and when is that useful?
4. Why is the floating route given 200 and not left at 1?
