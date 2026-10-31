# Lab 4 — Watch ARP, ICMP and DHCP in Simulation mode

**Course:** CCNA 1 (Introduction to Networks) · **Time:** 40 minutes

## Objectives

- Make a router hand out addresses with DHCP and watch the four DHCP messages a client and
  a server exchange.
- Watch a host resolve its gateway's MAC address with ARP before its first ping leaves.
- Follow an ICMP echo across a router in the event list and read what each device decided.

## Topology

```text
 PC1 ─┐
      S1 ──G0/1── (G0/0) R1 (G0/1) ── Server1
 PC2 ─┘
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 172.16.1.1/24 |
| R1 | | G0/1 | 172.16.2.1/24 |
| Server1 | Server | G0 | 172.16.2.10/24, gateway 172.16.2.1 |
| PC1, PC2 | PC | Fa0 | DHCP from R1 (pool 172.16.1.0/24, .1–.20 kept back) |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "Server", "name": "Server1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "Server1", "b_port": "g0"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
```

## Part 1 — A DHCP server on the router

```console R1
enable
configure terminal
interface g0/0
ip address 172.16.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 172.16.2.1 255.255.255.0
no shutdown
exit
ip dhcp excluded-address 172.16.1.1 172.16.1.20
ip dhcp pool LAN1
network 172.16.1.0 255.255.255.0
default-router 172.16.1.1
dns-server 172.16.2.10
domain-name lab.local
end
```

```console Server1
ipconfig 172.16.2.10 255.255.255.0 172.16.2.1
```

## Part 2 — DHCP, step by step

Switch to **Simulation** mode (bottom right) and clear the event list, then ask PC1 for an
address (Desktop → IP Configuration → DHCP, or its command prompt):

```lab
sim.clear_events {}
```

```console PC1
ipconfig /renew
```

The event list now holds the client's broadcast Discover, the server's Offer, the client's
Request and the server's Acknowledgement. Click each one to see the fields: the offered
address in `yiaddr`, and options 1 (mask), 3 (router), 6 (DNS) and 51 (lease time).

```verify
events: DHCP
events: DHCP DISCOVER
events: DHCP OFFER
events: DHCP REQUEST
events: DHCP ACK
PC1> ipconfig /all
  expect: 172.16.1.21
  expect: by DHCP
  expect: lab.local
R1# show ip dhcp binding
  expect: 172.16.1.21
```

```console PC2
ipconfig /renew
```

```verify
R1# show ip dhcp binding
  expect: 172.16.1.22
```

## Part 3 — ARP before the first echo

Empty PC1's ARP cache and the event list, then ping the server across the router:

```console PC1
arp -d
```

```lab
sim.clear_events {}
pdu.simple {"from": "PC1", "to": "172.16.2.10"}
```

The server is on another network, so PC1 asks for its **gateway's** MAC address, not the
server's: the first event is an ARP broadcast for 172.16.1.1. R1 answers, the echo request
leaves, and R1 in turn asks with ARP for the server's MAC on its other side.

```verify
events: ARP
events: ICMP
PC1> arp -a
  expect: 172.16.1.1
  reject: 172.16.2.10
R1# show arp
  expect: 172.16.2.10
  expect: 172.16.1.21
```

## Part 4 — Realtime again

Back in Realtime mode, every host reaches every other:

```verify
ping PC1 172.16.2.10 => success
ping PC2 172.16.1.21 => success
ping Server1 172.16.1.22 => success
```

## Reflection

1. Why is the DHCP Discover sent to the broadcast address, and from 0.0.0.0?
2. Why is the server's address not in PC1's ARP cache after the ping?
3. Which fields of the echo request change at R1, and which stay the same end to end?
