# Lab 18 — QoS: classify and mark at the edge, police in the core

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 45 minutes

## Objectives

- Classify traffic with class maps (by ACL and by protocol) and mark it with DSCP values at
  the edge of the network, as close to the source as it can be trusted.
- Police traffic in the provider's core by its marking: the voice class gets a guaranteed
  rate, everything else a small one with the excess dropped.
- Read the per-class counters and see a policed packet's drop reason.

## Topology

```text
 PC1 ── (G0/0) R1 (G0/1) ──── (G0/0) ISP (G0/1) ──── (G0/1) R2 (G0/0) ── Server1
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| R1 | | G0/1 | 203.0.113.1/30 |
| ISP | Router-2 | G0/0 | 203.0.113.2/30 |
| ISP | | G0/1 | 198.51.100.2/30 |
| R2 | Router-2 | G0/1 | 198.51.100.1/30 |
| R2 | | G0/0 | 192.168.2.1/24 |
| PC1 | PC | Fa0 | 192.168.1.10/24 |
| Server1 | Server (web on) | G0 | 192.168.2.10/24 |

| Class | Traffic | Marking |
|---|---|---|
| VOICE | UDP ports 16384–32767 (RTP) | EF (46) |
| WEB | HTTP and HTTPS | AF21 (18) |
| class-default | everything else | default (0) |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "Server", "name": "Server1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "ISP", "b_port": "g0/0"}
link.add {"a": "ISP", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "Server1", "b": "R2", "b_port": "g0/0"}
server.services {"device": "Server1", "services": {"http": {"on": true, "files": [{"name": "index.html", "content": "<h1>Branch portal</h1>"}]}}}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console Server1
ipconfig 192.168.2.10 255.255.255.0 192.168.2.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 203.0.113.1 255.255.255.252
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 203.0.113.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 192.168.2.1 255.255.255.0
no shutdown
interface g0/1
ip address 198.51.100.1 255.255.255.252
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 198.51.100.2
end
```

```console ISP
enable
configure terminal
interface g0/0
ip address 203.0.113.2 255.255.255.252
no shutdown
interface g0/1
ip address 198.51.100.2 255.255.255.252
no shutdown
exit
ip route 192.168.1.0 255.255.255.0 203.0.113.1
ip route 192.168.2.0 255.255.255.0 198.51.100.1
end
```

```verify
ping PC1 192.168.2.10 => success
```

## Part 1 — Classify and mark on R1

Marking happens once, inbound on the interface facing the users. Everything after R1 can
then classify on the DSCP value alone.

```console R1
configure terminal
access-list 110 permit udp any any range 16384 32767
class-map match-all VOICE
match access-group 110
class-map match-any WEB
match protocol http
match protocol https
exit
policy-map EDGE-MARK
class VOICE
set dscp ef
class WEB
set dscp af21
class class-default
set dscp default
exit
exit
interface g0/0
service-policy input EDGE-MARK
end
```

Send some "voice" (a UDP stream to an RTP port, from Simulation mode's Add Complex PDU) and
browse the server:

```lab
pdu.complex {"from": "PC1", "to": "192.168.2.10", "app": "udp", "dst_port": 16500}
browser.get {"device": "PC1", "url": "http://192.168.2.10/index.html"}
```

```verify
R1# show policy-map interface g0/0
  expect: Input policy EDGE-MARK
  expect: Class VOICE
  expect: set dscp ef
  expect: Class WEB
  expect: set dscp af21
R1# show class-map
  expect: VOICE
  expect: access-group 110
```

## Part 2 — Police in the provider's core

The ISP trusts the markings. Voice may use up to 128 kbit/s; the rest of this customer's
traffic is held to 8 kbit/s, and what exceeds it is dropped.

```console ISP
configure terminal
class-map match-any REALTIME
match dscp ef
exit
policy-map CUSTOMER-IN
class REALTIME
police 128000 conform-action transmit exceed-action drop
class class-default
police 8000 conform-action transmit exceed-action drop
exit
exit
interface g0/0
service-policy input CUSTOMER-IN
end
```

```lab
sim.clear_events {}
pdu.complex {"from": "PC1", "to": "192.168.2.10", "app": "udp", "dst_port": 16500}
```

```verify
ISP# show policy-map interface g0/0
  expect: Input policy CUSTOMER-IN
  expect: Class REALTIME
  expect: police 128000
  expect: police 8000
```

Large pings in a burst overflow the default class's bucket, and the excess is dropped, with
the reason in the event list:

```console PC1
ping 192.168.2.10 -l 1400
```

```verify
events: policed by policy-map CUSTOMER-IN
ISP# show policy-map interface g0/0
  expect: dropped
```

## Reflection

1. Why is traffic marked at R1's inbound interface rather than at the ISP?
2. What would happen to voice quality if the ISP policed every class at 8 kbit/s?
3. Policing drops the excess; what does shaping do instead, and where is each used?
