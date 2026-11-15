# Lab 38 — Capture traffic with port mirroring

**Course:** CCNA 2 follow-up (switch monitoring) · **Time:** 45 minutes

## Objectives

- Explain why a sniffer cabled to a switch port hears so little, and what a monitor session
  (SPAN) changes.
- Copy one port's traffic to a sniffer with `monitor session … source interface … ` and
  `monitor session … destination interface …`, and read it with `show monitor`.
- Choose what is copied with `rx`, `tx` and `both`, watch several ports with a range, and
  watch a whole VLAN with `source vlan`.
- Read the checks that keep a session sane: a destination is not also a source, one session per
  source, ports or VLANs but not both, and two sessions at once.
- Remove a session with the `no` forms, and read the session in the running configuration.

## Topology

```text
 PC1 ── Fa0/1 ─┐
 PC2 ── Fa0/2 ─┤
 PC3 ── Fa0/3 ─┼─ S1 ── Fa0/24 ── Sniffer1
```

| Device | Model | Port | Address |
|---|---|---|---|
| S1 | Switch-24 | | |
| PC1 | PC | S1 Fa0/1 | 10.38.0.1/24 |
| PC2 | PC | S1 Fa0/2 | 10.38.0.2/24 |
| PC3 | PC | S1 Fa0/3 | 10.38.0.3/24 |
| Sniffer1 | Sniffer | S1 Fa0/24 | — |

A sniffer records every frame that reaches its port and answers none of them (its **Capture**
tab lists them). In this lab you read the same thing from the event list: an event whose `to`
is `Sniffer1 FastEthernet0` is a frame the sniffer heard.

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
device.add {"model": "Sniffer", "name": "Sniffer1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "PC3", "b": "S1", "b_port": "fa0/3"}
link.add {"a": "Sniffer1", "b": "S1", "b_port": "fa0/24"}
```

```console PC1
ipconfig 10.38.0.1 255.255.255.0
```

```console PC2
ipconfig 10.38.0.2 255.255.255.0
```

```console PC3
ipconfig 10.38.0.3 255.255.255.0
```

## Part 1 — A sniffer on a plain switch

A switch sends a frame only where it is going. Once PC1 and PC2 have found each other the
echoes are unicast, so the sniffer on Fa0/24 hears none of them: only broadcasts such as ARP
reach every port.

```verify
ping PC1 10.38.0.2 => success
run sim.events {"limit": 10000}
  expect: "to":"Sniffer1 FastEthernet0"
  reject: ttl 128","to":"Sniffer1 FastEthernet0"
```

The first check shows the sniffer did hear the ARP broadcast; the second says that no IP
packet (an echo, here) ever reached it. Each IP summary in the event list ends with its TTL,
128 for these PCs.

## Part 2 — Copy one port to the sniffer

A monitor session names what to watch (`source`) and the one port that receives a copy
(`destination`). Watch PC1's port, in both directions:

```console S1
enable
configure terminal
monitor session 1 source interface fa0/1
monitor session 1 destination interface fa0/24
end
```

```verify
S1# show monitor
  expect: Monitor session 1 (local)
  expect: Source ports:  both Fa0/1
  expect: Destination:   Fa0/24 (link up)
  expect: State:         copying
```

Clear the event list, and let PC1 and PC2 talk again. The sniffer now hears the echo requests
PC1 sends (they enter the switch on Fa0/1) and the replies PC2 answers (they leave it on Fa0/1):

```lab
sim.clear_events {}
```

```verify
ping PC1 10.38.0.2 => success
run sim.events {"limit": 10000}
  expect: → 10.38.0.2 ttl 128","to":"Sniffer1 FastEthernet0"
  expect: → 10.38.0.1 ttl 128","to":"Sniffer1 FastEthernet0"
```

The first line is a request (to PC2), the second a reply (back to PC1). Traffic that does not
touch Fa0/1 is not copied. Clear the events and let PC2 and PC3 talk:

```lab
sim.clear_events {}
```

```verify
ping PC2 10.38.0.3 => success
run sim.events {"limit": 10000}
  reject: ttl 128","to":"Sniffer1 FastEthernet0"
```

A destination port carries nothing but copies: it forwards no other traffic, and it takes no
frames in. That is why the ARP broadcasts PC2 sent while finding PC3 reached the sniffer once
(copied from Fa0/1, where the switch flooded them) and not a second time as ordinary flooding.

## Part 3 — Only one direction

`rx` copies the frames the switch receives on the source port, `tx` the frames it sends out of
it, and `both` (the default) both. A source typed again changes its direction:

```console S1
configure terminal
monitor session 1 source interface fa0/1 rx
end
```

```lab
sim.clear_events {}
```

```verify
ping PC1 10.38.0.2 => success
run sim.events {"limit": 10000}
  expect: → 10.38.0.2 ttl 128","to":"Sniffer1 FastEthernet0"
  reject: → 10.38.0.1 ttl 128","to":"Sniffer1 FastEthernet0"
```

Only what PC1 sent (the requests) enters the switch on Fa0/1. With `tx` it is the other way
round: the replies leave the switch there.

```console S1
configure terminal
monitor session 1 source interface fa0/1 tx
end
```

```lab
sim.clear_events {}
```

```verify
ping PC1 10.38.0.2 => success
run sim.events {"limit": 10000}
  expect: → 10.38.0.1 ttl 128","to":"Sniffer1 FastEthernet0"
  reject: → 10.38.0.2 ttl 128","to":"Sniffer1 FastEthernet0"
```

## Part 4 — Several ports, and the running configuration

A range or a list of ports takes the same direction word. Watch all three PCs:

```console S1
configure terminal
monitor session 1 source interface fa0/1 - 3 both
end
```

```lab
sim.clear_events {}
```

```verify
ping PC2 10.38.0.3 => success
run sim.events {"limit": 10000}
  expect: → 10.38.0.3 ttl 128","to":"Sniffer1 FastEthernet0"
S1# show monitor session 1
  expect: Source ports:  both Fa0/1 - 3
S1# show running-config
  expect: monitor session 1 source interface FastEthernet0/1 - 3
  expect: monitor session 1 destination interface FastEthernet0/24
```

Now PC2's requests to PC3 are copied, and so are PC3's replies. The configuration shows the
session as typed: a run of neighbouring ports becomes a range.

## Part 5 — What a session refuses

The console checks the combinations a switch cannot honour. Each line below is typed in
configuration mode and refused with an explanation:

```console S1
configure terminal
```

```verify
S1# monitor session 1 source interface fa0/24
  expect: cannot also be a source
S1# monitor session 2 source interface fa0/24
  expect: cannot also be a source
S1# monitor session 1 destination interface fa0/1
  expect: cannot also be the destination
S1# monitor session 2 source interface fa0/1
  expect: already feeds session 1
S1# monitor session 1 source vlan 10
  expect: ports or VLANs
S1# monitor session 1 source vlan 1002
  expect: reserved
S1# monitor session 0 source interface fa0/9
  expect: 1-66
```

A source port belongs to one session, and so does a destination. A second session is fine
as long as it uses other ports; a third is not, because a switch holds two at once:

```console S1
monitor session 2 source interface fa0/9
monitor session 2 destination interface fa0/10
```

```verify
S1# monitor session 3 source interface fa0/11
  expect: holds 2 monitor sessions
S1# monitor session 2 destination interface fa0/24
  expect: already receives session 1
```

## Part 6 — A whole VLAN, and taking sessions away

`source vlan` copies every frame of a VLAN, whichever port it crosses. Replace session 1 with a
VLAN session (all the ports are in VLAN 1):

```console S1
no monitor session 2
no monitor session 1 source interface fa0/1 - 3
monitor session 1 source vlan 1 rx
end
```

```lab
sim.clear_events {}
```

```verify
ping PC2 10.38.0.3 => success
run sim.events {"limit": 10000}
  expect: → 10.38.0.3 ttl 128","to":"Sniffer1 FastEthernet0"
S1# show monitor
  expect: Source ports:  none
  expect: Source VLANs:  rx 1
  expect: Destination:   Fa0/24 (link up)
```

`no monitor session all` deletes every session; the sniffer goes back to hearing only what a
switch floods to everyone.

```console S1
configure terminal
no monitor session all
end
```

```lab
sim.clear_events {}
```

```verify
S1# show monitor
  expect: No monitor session is configured.
ping PC2 10.38.0.3 => success
run sim.events {"limit": 10000}
  reject: ttl 128","to":"Sniffer1 FastEthernet0"
```

Save the lab with a session in place and open it again: the sessions are part of the file, and
`copy running-config startup-config` keeps them across a `reload`.
