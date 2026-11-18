# Lab 64 — Troubleshooting: IP phones that will not register

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials), voice topics · **Time:** 60 minutes

## Objectives

- Build a small call-control setup: a PoE switch with a data VLAN and a voice VLAN, a router that
  hands out addresses (with the call server's address in option 150) and serves as the call
  server (`telephony-service`, `ephone-dn`, `auto assign`).
- Find two reasons a phone does not register: it is in the wrong VLAN and never learns where the
  call server is, and the call server is told to accept one phone only.
- Read what the phone says (`phone.status`), what the call server says (`show ephone`,
  `show ephone-dn`, `show telephony-service`), and place a call.

## Topology

```text
                 ┌── Fa0/1 ── Phone1
 R1 ── G0/1 ── S1 ┤
                 └── Fa0/2 ── Phone2
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0.10 | 192.168.10.1/24 (data, VLAN 10) |
| R1 | | G0/0.100 | 192.168.100.1/24 (voice, VLAN 100) |
| S1 | Switch-24-PoE | G0/1 | trunk to R1 |
| Phone1, Phone2 | IP-Phone | Fa0 | from DHCP |

The phones get power from the switch. Each takes an address in the voice VLAN and learns from
DHCP option 150 where the call server is (R1's voice address, 192.168.100.1).

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24-PoE", "name": "S1"}
device.add {"model": "IP-Phone", "name": "Phone1"}
device.add {"model": "IP-Phone", "name": "Phone2"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "Phone1", "a_port": "fa0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "Phone2", "a_port": "fa0", "b": "S1", "b_port": "fa0/2"}
```

## Part 1 — The network, with two mistakes in it

The switch is configured from a note that forgot the voice VLAN on Fa0/2. The router is set
up to accept one phone, which is a left-over of a first test.

```console S1
enable
configure terminal
vlan 10
name DATA
vlan 100
name VOICE
exit
interface fa0/1
switchport mode access
switchport access vlan 10
switchport voice vlan 100
interface fa0/2
switchport mode access
switchport access vlan 10
interface g0/1
switchport mode trunk
end
```

```console R1
enable
configure terminal
interface g0/0
no shutdown
interface g0/0.10
encapsulation dot1Q 10
ip address 192.168.10.1 255.255.255.0
interface g0/0.100
encapsulation dot1Q 100
ip address 192.168.100.1 255.255.255.0
exit
ip dhcp excluded-address 192.168.10.1
ip dhcp excluded-address 192.168.100.1
ip dhcp pool DATA
network 192.168.10.0 255.255.255.0
default-router 192.168.10.1
ip dhcp pool VOICE
network 192.168.100.0 255.255.255.0
default-router 192.168.100.1
option 150 ip 192.168.100.1
exit
telephony-service
max-ephones 1
max-dn 4
ip source-address 192.168.100.1 port 2000
auto assign 1 to 4
exit
ephone-dn 1
number 1001
ephone-dn 2
number 1002
end
```

```console Phone1
ipconfig /renew
```

```console Phone2
ipconfig /renew
```

```lab
sim.advance {"seconds": 5}
```

## Part 2 — Phone2: in the wrong VLAN

Phone1 registered; Phone2 did not.

```verify
run phone.status {"device": "Phone1"}
  expect: "registered":true
  expect: "number":"1001"
run phone.status {"device": "Phone2"}
  expect: "registered":false
R1# show ephone
  expect: registered (auto assign)
  expect: 1001
  reject: 1002
```

A phone that does not register is either not reaching the call server or being refused by
it. The call server has not refused anything (there is no message), so start from what the
phone has: its address and where it was told to register.

```verify
run phone.status {"device": "Phone2"}
  expect: "voice_vlan":null
run phone.status {"device": "Phone1"}
  expect: "voice_vlan":100
S1# show interfaces fa0/1 switchport
  expect: Voice VLAN: 100
S1# show interfaces fa0/2 switchport
  reject: Voice VLAN
```

Phone1's port has a voice VLAN and Phone2's has none: Phone2 sends its frames untagged into
the data VLAN, where the DHCP pool has no option 150, so it has an address and no call server.
Give the port the voice VLAN, and renew the phone's address, because it learns the call server
only when it asks DHCP again.

```console S1
configure terminal
interface fa0/2
switchport voice vlan 100
end
```

```console Phone2
ipconfig /renew
```

```lab
sim.advance {"seconds": 5}
```

## Part 3 — Phone2: refused by the call server

Phone2 now reaches the call server, and the call server turns it away. This time there is a
reason, on the phone's screen and in the router's counters.

```verify
run phone.status {"device": "Phone2"}
  expect: "registered":false
  expect: registration refused: max ephones reached
R1# show telephony-service
  expect: Registered phones:    1
```

`max-ephones 1` is a limit, and Phone1 has used it. Raise it, and let Phone2 try again.

```console R1
configure terminal
telephony-service
max-ephones 2
end
```

```console Phone2
ipconfig /renew
```

```lab
sim.advance {"seconds": 5}
```

```verify
run phone.status {"device": "Phone2"}
  expect: "registered":true
  expect: "number":"1002"
R1# show ephone
  expect: 1001
  expect: 1002
R1# show ephone-dn
  expect: 1001
  expect: 1002
```

## Part 4 — Place a call

Phone1 dials Phone2. Dialing a number nobody has ends the call at once and says why.

```verify
run phone.dial {"device": "Phone1", "number": "1009"}
  expect: unknown
run phone.dial {"device": "Phone1", "number": "1002"}
  expect: "state":"ringback"
run phone.status {"device": "Phone2"}
  expect: "state":"ringing"
run phone.answer {"device": "Phone2"}
  expect: "state":"connected"
run phone.hangup {"device": "Phone1"}
  reject: "state":"connected"
```

## Reflection

1. What is the difference between a phone that does not register and says nothing, and one that
   is refused and says so? Which one is a network problem?
2. Why must the phone ask DHCP again after the voice VLAN is fixed?
3. What does `auto assign 1 to 4` save you from typing, and what does it not decide?
4. A third phone is added to Fa0/3. Which of the two limits you found would it meet first?
