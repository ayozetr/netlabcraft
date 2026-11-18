# Lab 86 — Storm control and UDLD: keeping a bad port or a half-dead cable from hurting the LAN

**Course:** CCNA 2 follow-up (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Limit broadcast, multicast and unknown-unicast traffic on a port with `storm-control`, in percent of
  the link, bits per second or packets per second.
- Choose what happens above the level: drop the excess, log it (`trap`) or err-disable the port (`shutdown`).
- Read `show storm-control` and bring a port back after a shutdown.
- Turn on UDLD, cut one strand of a cable so that it works in one direction only, and see UDLD
  shut the port that the link is wrong for.

## Topology

```text
 PC1 ──Fa0/1── S1 ──Gi0/1════Gi0/1── S2 ── PC2
```

| Device | Model | Interface | Role |
|---|---|---|---|
| S1 | Switch-24 | Fa0/1 | PC1, with storm control |
| S1 | | Gi0/1 | link to S2, with UDLD |
| S2 | Switch-24 | Gi0/1 | link to S1, with UDLD |
| S2 | | Fa0/1 | PC2 |
| PC1 | PC | Fa0 | 10.86.0.10/24 |
| PC2 | PC | Fa0 | 10.86.0.20/24 |

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Switch-24", "name": "S2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S2", "b_port": "fa0/1"}
link.add {"a": "S1", "a_port": "g0/1", "b": "S2", "b_port": "g0/1"}
```

```console PC1
ipconfig 10.86.0.10 255.255.255.0
```

```console PC2
ipconfig 10.86.0.20 255.255.255.0
```

## Part 1 — A level for broadcasts

The level is what a port may take in each second. Here it is 50 broadcast frames a second, and a
second level (20) says that the storm lasts until a second has no more than that. The default
action drops the excess; the port stays up.

```console S1
enable
configure terminal
interface fa0/1
storm-control broadcast level pps 50 20
storm-control multicast level 5
end
```

```verify
S1# show storm-control
  expect: Fa0/1
  expect: broadcast
  expect: 50 pps
  expect: 20 pps
  expect: 5%
S1# show running-config
  expect: storm-control broadcast level pps 50 20
  expect: storm-control multicast level 5
```

A percent level is a share of the link's speed (a fast Ethernet port takes 100 Mbps, so 5% is 5 Mbps);
`bps` and `pps` levels do not depend on the link. Unicast storm control counts only frames to a destination
the switch has not learned.

## Part 2 — What happens above the level

`storm-control action trap` logs the first excess of a storm; `action shutdown` err-disables the port, as BPDU
guard does. An err-disabled port stays down until `shutdown` and `no shutdown`, or until errdisable recovery
is asked for with the cause `storm-control`.

```console S1
configure terminal
interface fa0/1
storm-control action shutdown
exit
errdisable recovery cause storm-control
end
```

```verify
S1# show storm-control
  expect: shutdown
S1# show errdisable recovery
  expect: storm-control
```

## Part 3 — UDLD

UDLD sends a probe from each port every few seconds (15 by default) and listens for the probes of the
port at the other end. A link where the other end does not hear us is wrong, even though the port stays up.

```console S1
configure terminal
udld enable
udld message time 5
end
```

```console S2
enable
configure terminal
udld enable
udld message time 5
end
```

```lab
sim.advance {"seconds": 30}
```

```verify
S1# show udld
  expect: Gi0/1
  expect: bidirectional
  expect: S2
```

Now cut the strand S1 sends on. S2 still sends, so S1 hears S2, but S2 does not hear S1:

```lab
link.one_way {"device": "S1", "port": "g0/1"}
sim.advance {"seconds": 60}
```

```verify
S1# show interfaces status err-disabled
  expect: udld
S1# show udld g0/1
  expect: err-disabled by UDLD
```

On a real cable the port would stay up and forward in one direction, which is how a loop can form. Put the strand
back, bring the port up and read the state again:

```lab
link.one_way {"device": "S1", "port": "g0/1", "cut": false}
```

```console S1
udld reset
```

```lab
sim.advance {"seconds": 40}
```

```verify
S1# show udld g0/1
  expect: bidirectional
```

## Questions

1. Why does `storm-control` count multicast and broadcast frames apart?
2. A port whose UDLD mode is `aggressive` also shuts when its neighbour goes silent. When is that better than normal mode, and when is it worse?
3. What would you set to log a storm without ever shutting the port?
